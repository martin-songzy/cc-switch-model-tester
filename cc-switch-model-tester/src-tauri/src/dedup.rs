//! 测试目标展开与精确去重（DevelopmentPlan.md 5.8 / 10.1 / 14.1）。
//!
//! 只允许对「端点、模型、协议、测试模式、认证和有效请求配置均相同」的目标去重；
//! 不按供应商名称或 ID 粗略合并。

use std::collections::{HashMap, HashSet};

use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::{
    ApiProtocol, AppType, ProviderSnapshot, TestMode, TestTarget,
};

/// 一个测试批次的前端输入（文档 14.1）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunInput {
    pub app: AppType,
    pub provider_ids: Vec<String>,
    pub model_keys: Vec<String>,
    pub attempts_per_model: u32,
    pub mode: TestMode,
    pub global_concurrency: u32,
    pub provider_concurrency: u32,
    pub test_all_candidate_endpoints: bool,
    #[serde(default)]
    pub apply_body_overrides: bool,
    /// 每模型客户端仿真选择（key = "app::providerId::modelId"；值为画像名或 ""=关闭；
    /// 缺省 = 供应商配置 enabled ? 配置 profile : 关闭）
    #[serde(default)]
    pub emulation_overrides: HashMap<String, String>,
    /// 单次请求总超时（秒），默认 60
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
}

fn default_timeout() -> u32 {
    60
}

/// 目标来源引用（历史/去重展示用，不含凭据）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetSourceRef {
    pub app: AppType,
    pub provider_id: String,
    pub provider_name: String,
    pub model_key: String,
}

/// 展开后的原始目标（含凭据，仅 Rust 内存）。
#[derive(Debug, Clone)]
pub struct ExpandedTarget {
    pub target: TestTarget,
    pub source: TargetSourceRef,
}

/// 去重组：代表项 + 被合并来源。
#[derive(Debug, Clone)]
pub struct DedupGroup {
    pub key_hash: String,
    pub representative: TestTarget,
    pub merged_sources: Vec<TargetSourceRef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupSummary {
    pub original_target_count: usize,
    pub deduplicated_target_count: usize,
    pub original_attempt_count: usize,
    pub deduplicated_attempt_count: usize,
    pub duplicate_group_count: usize,
    pub removed_attempt_count: usize,
}

/// 去重确认框的展示结构（脱敏）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupGroupView {
    pub key_hash: String,
    pub representative: TargetDisplay,
    pub merged_sources: Vec<TargetSourceRef>,
    pub removed_target_count: usize,
    pub removed_attempt_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDisplay {
    pub app: AppType,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub endpoint_display: String,
    pub protocol: ApiProtocol,
    pub mode: TestMode,
    /// 客户端仿真（开启时为画像名）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emulation_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupPreview {
    pub preview_id: String,
    pub summary: DedupSummary,
    pub groups: Vec<DedupGroupView>,
    /// 每个标签页跳过的供应商（配置错误 / 托管 / 暂不支持）
    pub skipped: Vec<SkippedProviderView>,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedProviderView {
    pub provider_id: String,
    pub provider_name: String,
    pub reason: String,
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// 凭据指纹：只暴露不可逆哈希前 16 位。
fn fingerprint(secret: &str) -> String {
    let full = sha256_hex(secret.as_bytes());
    full[..16].to_string()
}

fn is_auth_header(name: &str) -> bool {
    let n = name.to_lowercase();
    n == "authorization" || n == "x-api-key" || n == "x-goog-api-key"
}

fn protocol_str(p: ApiProtocol) -> &'static str {
    match p {
        ApiProtocol::AnthropicMessages => "anthropic_messages",
        ApiProtocol::OpenAiChat => "openai_chat",
        ApiProtocol::OpenAiResponses => "openai_responses",
        ApiProtocol::GeminiNative => "gemini_native",
        ApiProtocol::BedrockConverseStream => "bedrock",
    }
}

/// 计算精确去重键（文档 5.8 全部字段）。
pub fn dedup_key_hash(t: &TestTarget, mode: TestMode) -> String {
    let mut material = String::new();

    // 1. 规范化端点（保留 query）
    material.push_str(
        &crate::protocol::url::normalize_base(&t.endpoint_url).unwrap_or_else(|_| t.endpoint_url.clone()),
    );
    material.push('|');
    // 2. 协议
    material.push_str(protocol_str(t.protocol));
    material.push('|');
    // 3. 实际模型 ID
    material.push_str(&t.model_id);
    material.push('|');
    // 4. 测试模式
    material.push_str(match mode {
        TestMode::NonStreaming => "non_streaming",
        TestMode::Streaming => "streaming",
    });
    material.push('|');
    // 5. 凭据形态 + 指纹（值不进入键）
    match &t.credential {
        Some(c) => {
            let kind = match c.kind {
                crate::domain::CredentialKind::AuthToken => "auth_token",
                crate::domain::CredentialKind::ApiKeyHeader => "api_key_header",
                crate::domain::CredentialKind::BearerKey => "bearer_key",
                crate::domain::CredentialKind::GeminiKey => "gemini_key",
                crate::domain::CredentialKind::ExplicitHeader => "explicit_header",
                crate::domain::CredentialKind::Managed => "managed",
                crate::domain::CredentialKind::Missing => "missing",
            };
            material.push_str(kind);
            material.push(':');
            material.push_str(&fingerprint(&c.secret));
        }
        None => material.push_str("none"),
    }
    material.push('|');
    // 6. 有效请求 Header（名称小写排序；认证值以指纹参与）
    let mut hdrs: Vec<String> = t
        .headers
        .iter()
        .map(|(k, v)| {
            let vv = if is_auth_header(k) {
                fingerprint(v)
            } else {
                v.clone()
            };
            format!("{}={}", k.to_lowercase(), vv)
        })
        .collect();
    hdrs.sort();
    material.push_str(&hdrs.join(";"));
    material.push('|');
    // 7. 自定义 UA
    material.push_str(t.custom_user_agent.as_deref().unwrap_or(""));
    material.push('|');
    // 8. full_url
    material.push_str(if t.full_url { "full" } else { "auto" });
    material.push('|');
    // 8.5 客户端仿真状态（实际生效的画像名；未启用则空）
    material.push_str(
        crate::emulation::resolve_profile(t)
            .map(|p| p.name)
            .unwrap_or(""),
    );
    material.push('|');
    // 9. compat 规范序列化（serde_json 对象键按字母序，确定性）
    material.push_str(&t.compat.to_string());
    // 10. body override hash：M4 未启用 Body 覆盖，恒定值

    sha256_hex(material.as_bytes())
}

/// 展开一个供应商快照为测试目标列表。
#[allow(clippy::too_many_arguments)]
pub fn expand_provider(
    snap: &ProviderSnapshot,
    selected_models: &HashSet<String>,
    attempts_per_model: u32,
    mode: TestMode,
    test_all_candidate_endpoints: bool,
    emulation_overrides: &HashMap<String, String>,
) -> Vec<ExpandedTarget> {
    if snap.status != crate::domain::ProviderStatus::Ready {
        return Vec::new();
    }
    let mut out = Vec::new();
    for model in &snap.models {
        if !selected_models.is_empty() && !selected_models.contains(&model.model_id) {
            continue;
        }
        let mut endpoints: Vec<String> = vec![model
            .base_url_override
            .clone()
            .unwrap_or_else(|| snap.endpoint.clone())];
        if test_all_candidate_endpoints {
            for c in &snap.candidate_endpoints {
                if !endpoints.contains(c) {
                    endpoints.push(c.clone());
                }
            }
        }
        for ep in endpoints {
            let mut t = snap.to_test_target(model);
            t.endpoint_url = ep;
            // 每模型仿真选择：前端显式选择（画像名/空=关）> 供应商配置默认值（enabled）
            let model_key = format!("{}::{}::{}", snap.app.as_str(), snap.provider_id, model.model_id);
            let chosen = emulation_overrides
                .get(&model_key)
                .cloned()
                .unwrap_or_else(|| {
                    snap.client_emulation
                        .as_ref()
                        .filter(|s| s.enabled)
                        .map(|s| s.profile.clone())
                        .unwrap_or_default()
                });
            t.emulation = !chosen.is_empty();
            if t.emulation {
                t.emulation_profile = Some(chosen);
            }
            let source = TargetSourceRef {
                app: snap.app,
                provider_id: snap.provider_id.clone(),
                provider_name: snap.provider_name.clone(),
                model_key: format!("{}::{}", snap.provider_id, model.model_id),
            };
            // attempts_per_model 在调度阶段展开；这里每个 (端点, 模型) 是一个目标
            let _ = attempts_per_model;
            let _ = mode;
            out.push(ExpandedTarget { target: t, source });
        }
    }
    out
}

/// 对展开目标执行确定性去重（文档 5.8 稳定排序）。
pub fn deduplicate(
    targets: Vec<ExpandedTarget>,
    mode: TestMode,
    attempts_per_model: u32,
) -> (Vec<DedupGroup>, DedupSummary) {
    // 稳定顺序：供应商名称、provider ID、模型 ID（标签页顺序天然单批次内一致）
    let mut sorted = targets;
    sorted.sort_by(|a, b| {
        a.source
            .provider_name
            .cmp(&b.source.provider_name)
            .then(a.source.provider_id.cmp(&b.source.provider_id))
            .then(a.target.model_id.cmp(&b.target.model_id))
    });

    let mut map: HashMap<String, DedupGroup> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let original_targets = sorted.len();

    for item in sorted {
        let key = dedup_key_hash(&item.target, mode);
        match map.get_mut(&key) {
            Some(g) => g.merged_sources.push(item.source),
            None => {
                order.push(key.clone());
                map.insert(
                    key,
                    DedupGroup {
                        key_hash: dedup_key_hash(&item.target, mode),
                        representative: item.target,
                        merged_sources: vec![item.source],
                    },
                );
            }
        }
    }

    let groups: Vec<DedupGroup> = order
        .into_iter()
        .filter_map(|k| map.remove(&k))
        .collect();

    let dedup_targets = groups.len();
    let original_attempts = original_targets as u32 * attempts_per_model;
    let dedup_attempts = dedup_targets as u32 * attempts_per_model;
    let summary = DedupSummary {
        original_target_count: original_targets,
        deduplicated_target_count: dedup_targets,
        original_attempt_count: original_attempts as usize,
        deduplicated_attempt_count: dedup_attempts as usize,
        duplicate_group_count: groups
            .iter()
            .filter(|g| g.merged_sources.len() > 1)
            .count(),
        removed_attempt_count: (original_attempts - dedup_attempts) as usize,
    };
    (groups, summary)
}

/// 构造去重预览（脱敏展示结构）。
pub fn build_preview(
    preview_id: String,
    groups: &[DedupGroup],
    summary: &DedupSummary,
    mode: TestMode,
    attempts_per_model: u32,
    skipped: Vec<SkippedProviderView>,
    expires_at: i64,
) -> DedupPreview {
    let duplicate_groups = groups
        .iter()
        .filter(|g| g.merged_sources.len() > 1)
        .map(|g| {
            let rep_display = TargetDisplay {
                app: g.representative.app,
                provider_id: g.representative.provider_id.clone(),
                provider_name: g.representative.provider_name.clone(),
                model_id: g.representative.model_id.clone(),
                endpoint_display: crate::redact::redact_url(&g.representative.endpoint_url),
                protocol: g.representative.protocol,
                mode,
                emulation_profile: if g.representative.emulation {
                    g.representative
                        .client_emulation
                        .as_ref()
                        .map(|s| s.profile.clone())
                        .or_else(|| {
                            crate::emulation::resolve_profile(&g.representative)
                                .map(|p| p.name.to_string())
                        })
                } else {
                    None
                },
            };
            let merged = g.merged_sources.len() as u32 - 1;
            DedupGroupView {
                key_hash: g.key_hash.clone(),
                representative: rep_display,
                merged_sources: g.merged_sources.clone(),
                removed_target_count: merged as usize,
                removed_attempt_count: (merged * attempts_per_model) as usize,
            }
        })
        .collect();
    DedupPreview {
        preview_id,
        summary: summary.clone(),
        groups: duplicate_groups,
        skipped,
        expires_at,
    }
}

/// 生成 preview id。
pub fn gen_preview_id(rng: &mut impl Rng) -> String {
    (0..16)
        .map(|_| format!("{:x}", rng.random_range(0u8..16)))
        .collect()
}
/// 模型快照辅助：把 model_keys 过滤为该供应商的选中集合。
/// key 格式：`{app}::{provider_id}::{model_id}`（app 前缀隔离跨标签页同名供应商/模型）。
pub fn selected_models_for(
    app: &str,
    provider_id: &str,
    model_keys: &[String],
) -> std::collections::HashSet<String> {
    let prefix = format!("{app}::{provider_id}::");
    model_keys
        .iter()
        .filter(|k| k.starts_with(&prefix))
        .map(|k| k[prefix.len()..].to_string())
        .collect()
}

// 测试见 tests/dedup_tests.rs
