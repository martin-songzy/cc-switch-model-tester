//! cc-switch 供应商配置解析器。
//!
//! 输入是 cc-switch `providers.settings_config` 与 `providers.meta` 的 JSON 文本，
//! 输出统一的 `ProviderSnapshot`。规则依据 DevelopmentPlan.md 第 6 节。

pub mod claude;
pub mod codex;
pub mod pi;

use crate::domain::{
    config_hash, AppType, CredentialKind, CredentialValue, ModelSnapshot, ProviderSnapshot,
    ProviderStatus, ApiProtocol,
};

/// 托管认证明细检测（DevelopmentPlan.md 5.3 / provider.rs 精确语义）：
/// - meta.providerType ∈ { codex_oauth, xai_oauth, github_copilot }
/// - Claude baseUrl 含 githubcopilot.com
/// - 任意 baseUrl 含 chatgpt.com/backend-api/codex
pub(super) fn detect_managed(
    meta: &serde_json::Value,
    endpoint: Option<&str>,
) -> bool {
    if let Some(pt) = meta.get("providerType").and_then(|v| v.as_str()) {
        if matches!(pt, "codex_oauth" | "xai_oauth" | "github_copilot") {
            return true;
        }
    }
    if let Some(url) = endpoint {
        let lower = url.to_lowercase();
        if lower.contains("githubcopilot.com")
            || lower.contains("chatgpt.com/backend-api/codex")
        {
            return true;
        }
    }
    false
}

/// 从对象中收集字符串键值对为 Header 列表（跳过非字符串值）。
pub(super) fn collect_headers(v: Option<&serde_json::Value>) -> Vec<(String, String)> {
    let Some(obj) = v.and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (k, val) in obj {
        if let Some(s) = val.as_str() {
            out.push((k.clone(), s.to_string()));
        }
    }
    // 稳定排序，保证去重键确定性
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

/// 解析入口。任何失败都体现为 status=ConfigError 的快照，不 panic。
#[allow(clippy::too_many_arguments)]
pub fn parse_provider(
    app: AppType,
    provider_id: &str,
    provider_name: &str,
    settings_config: &str,
    meta: &str,
    candidate_endpoints: Vec<String>,
) -> ProviderSnapshot {
    let raw_hash = config_hash(settings_config);

    let sc: serde_json::Value = match serde_json::from_str(settings_config) {
        Ok(v) => v,
        Err(e) => {
            return error_snapshot(
                app,
                provider_id,
                provider_name,
                raw_hash,
                candidate_endpoints,
                format!("供应商配置 JSON 无效: {e}"),
            )
        }
    };
    let meta_v: serde_json::Value =
        serde_json::from_str(meta).unwrap_or(serde_json::Value::Null);

    match app {
        AppType::Claude => {
            claude::parse(provider_id, provider_name, &sc, &meta_v, candidate_endpoints, raw_hash)
        }
        AppType::Codex => {
            codex::parse(provider_id, provider_name, &sc, &meta_v, candidate_endpoints, raw_hash)
        }
        AppType::Pi => {
            pi::parse(provider_id, provider_name, &sc, &meta_v, candidate_endpoints, raw_hash)
        }
    }
}

/// 构造一个 ConfigError 快照。
pub(super) fn error_snapshot(
    app: AppType,
    provider_id: &str,
    provider_name: &str,
    raw_hash: String,
    candidate_endpoints: Vec<String>,
    message: String,
) -> ProviderSnapshot {
    ProviderSnapshot {
        app,
        provider_id: provider_id.to_string(),
        provider_name: provider_name.to_string(),
        endpoint: String::new(),
        candidate_endpoints,
        protocol: ApiProtocol::AnthropicMessages, // 占位；ConfigError 不发起请求
        raw_protocol: None,
        models: Vec::new(),
        credential: None,
        headers: Vec::new(),
        custom_user_agent: None,
        full_url: false,
        compat: serde_json::Value::Null,
        passthrough: serde_json::Value::Null,
        raw_config_hash: raw_hash,
        status: ProviderStatus::ConfigError,
        warnings: Vec::new(),
        config_error: Some(message),
    }
}

/// 便捷构造：凭据缺失错误。
pub(super) fn missing_credential_error(msg: &str) -> String {
    format!("缺少认证信息: {msg}")
}

#[allow(dead_code)]
fn credential_kind_of(_c: &CredentialValue) -> CredentialKind {
    _c.kind
}

/// 从 JSON 对象取非空字符串。
pub(super) fn nonempty_str<'a>(v: Option<&'a serde_json::Value>) -> Option<&'a str> {
    v.and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty())
}

/// 模型快照便捷构造（带从 ID 剥离的展示标记）。
pub(super) fn model_with_markers(id: impl Into<String>, markers: Vec<String>) -> ModelSnapshot {
    ModelSnapshot {
        model_id: id.into(),
        display_name: None,
        base_url_override: None,
        compat: serde_json::Value::Null,
        thinking_level_map: None,
        id_markers: markers,
    }
}
