//! Claude Code 供应商解析（DevelopmentPlan.md 6.1）。
//!
//! settings_config 形如：
//! ```json
//! { "env": { "ANTHROPIC_BASE_URL": "...", "ANTHROPIC_AUTH_TOKEN": "..." },
//!   "api_format": "anthropic" }
//! ```
//! meta 形如：
//! ```json
//! { "apiFormat": "anthropic", "isFullUrl": false, "customUserAgent": "...",
//!   "localProxyRequestOverrides": { "headers": { }, "body": { } },
//!   "stackModels": [ { "model": "id", "displayName": "...", "oneM": true } ] }
//! ```
//! 模型集合与 cc-switch 4.0 一致（上游 src-tauri/src/mode/stack.rs `claude_models`）：
//! - meta.stackModels 是数组 → 只认该列表（界面「模型列表」），空列表 = 没有模型；
//!   删除模型只改列表、不清 env 档位旧值，故此时必须忽略 env；
//! - 未配列表 → env 模型映射 ANTHROPIC_MODEL + OPUS/SONNET/HAIKU/FABLE 四档，
//!   显示名取对应档位的 *_MODEL_NAME。

use serde_json::Value;

use crate::domain::{
    ApiProtocol, AppType, CredentialKind, CredentialValue, ProviderSnapshot, ProviderStatus,
};

use super::{collect_headers, detect_managed, error_snapshot, nonempty_str};

/// env 模型映射（模型字段, 显示名字段），顺序与 cc-switch 一致。
const MODEL_ENV_KEYS: [(&str, Option<&str>); 5] = [
    ("ANTHROPIC_MODEL", None),
    ("ANTHROPIC_DEFAULT_OPUS_MODEL", Some("ANTHROPIC_DEFAULT_OPUS_MODEL_NAME")),
    ("ANTHROPIC_DEFAULT_SONNET_MODEL", Some("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME")),
    ("ANTHROPIC_DEFAULT_HAIKU_MODEL", Some("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME")),
    ("ANTHROPIC_DEFAULT_FABLE_MODEL", Some("ANTHROPIC_DEFAULT_FABLE_MODEL_NAME")),
];

/// 凭据优先级（文档 6.1）：AUTH_TOKEN > API_KEY > OPENROUTER > OPENAI > GEMINI > 顶层 apiKey/api_key
const CREDENTIAL_ENV_KEYS: [(&str, CredentialKind); 5] = [
    ("ANTHROPIC_AUTH_TOKEN", CredentialKind::AuthToken),
    ("ANTHROPIC_API_KEY", CredentialKind::ApiKeyHeader),
    ("OPENROUTER_API_KEY", CredentialKind::BearerKey),
    ("OPENAI_API_KEY", CredentialKind::BearerKey),
    ("GEMINI_API_KEY", CredentialKind::GeminiKey),
];

pub(super) fn parse(
    provider_id: &str,
    provider_name: &str,
    sc: &Value,
    meta: &Value,
    candidate_endpoints: Vec<String>,
    raw_hash: String,
) -> ProviderSnapshot {
    let mut warnings: Vec<String> = Vec::new();

    // ---- 托管认证检测 ----
    let env = sc.get("env").and_then(|v| v.as_object());
    let base_url_raw = env
        .and_then(|e| nonempty_str(e.get("ANTHROPIC_BASE_URL")))
        .map(|s| s.trim().to_string());
    if detect_managed(meta, base_url_raw.as_deref()) {
        return managed_snapshot(provider_id, provider_name, raw_hash, candidate_endpoints);
    }

    // ---- 协议判定：meta.apiFormat > settings_config.api_format > legacy openrouter_compat_mode > anthropic ----
    let raw_fmt = nonempty_str(meta.get("apiFormat")).or_else(|| nonempty_str(sc.get("api_format")));
    let legacy_openrouter = matches!(sc.get("openrouter_compat_mode"), Some(Value::Bool(true)))
        || nonempty_str(sc.get("openrouter_compat_mode")) == Some("openrouter");
    let raw_protocol_str = raw_fmt
        .map(|s| s.to_string())
        .or_else(|| legacy_openrouter.then(|| "openrouter_compat_mode".to_string()));

    let protocol = match raw_fmt.or_else(|| legacy_openrouter.then_some("openai_chat")) {
        Some("anthropic") => ApiProtocol::AnthropicMessages,
        Some("openai_chat") => ApiProtocol::OpenAiChat,
        Some("openai_responses") => ApiProtocol::OpenAiResponses,
        Some("gemini_native") => ApiProtocol::GeminiNative,
        Some(other) => {
            return error_snapshot(
                AppType::Claude,
                provider_id,
                provider_name,
                raw_hash,
                candidate_endpoints,
                format!(
                    "未知的 Claude 协议值 \"{other}\"，按配置错误处理（合法值: anthropic/openai_chat/openai_responses/gemini_native）"
                ),
            )
        }
        None => ApiProtocol::AnthropicMessages,
    };

    // ---- Base URL ----
    let Some(endpoint) = base_url_raw else {
        return error_snapshot(
            AppType::Claude,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            "缺少 Base URL（env.ANTHROPIC_BASE_URL 为空）".to_string(),
        );
    };

    // ---- 模型集合：stackModels 列表优先，未配时用 env 映射（规则见文件头）----
    // 按剥离 [1M] 类标记后的 id 去重并合并 markers（如 HAIKU=x 与 SONNET=x[1M] 指向同一模型，
    // 应保留 [1M] 能力标记，否则会丢失 1M 上下文 beta 注入）
    let mut models = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let stack_list = meta.get("stackModels").and_then(|v| v.as_array());
    if let Some(stack) = stack_list {
        // 条目 { model, displayName?, oneM? }，oneM=true 等价于 [1M] 标记
        for m in stack {
            if let Some(raw) = nonempty_str(m.get("model")) {
                let one_m = m.get("oneM").and_then(|v| v.as_bool()).unwrap_or(false);
                push_model(&mut models, &mut index, raw, nonempty_str(m.get("displayName")), one_m);
            }
        }
    } else if let Some(env) = env {
        for (key, name_key) in MODEL_ENV_KEYS {
            if let Some(raw) = nonempty_str(env.get(key)) {
                let name = name_key.and_then(|k| nonempty_str(env.get(k)));
                push_model(&mut models, &mut index, raw, name, false);
            }
        }
    }
    if models.is_empty() {
        let msg = if stack_list.is_some() {
            "模型列表为空（cc-switch 中该供应商的「模型列表」已清空）"
        } else {
            "模型列表为空（未配模型列表，且 env 中未找到模型映射字段）"
        };
        return error_snapshot(
            AppType::Claude,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            msg.to_string(),
        );
    }

    // ---- 凭据（按文档优先级）----
    let mut credential: Option<CredentialValue> = None;
    if let Some(env) = env {
        for (key, kind) in CREDENTIAL_ENV_KEYS {
            if let Some(secret) = nonempty_str(env.get(key)) {
                credential = Some(CredentialValue {
                    kind,
                    secret: secret.trim().to_string(),
                });
                break;
            }
        }
    }
    if credential.is_none() {
        let top = nonempty_str(sc.get("apiKey")).or_else(|| nonempty_str(sc.get("api_key")));
        if let Some(secret) = top {
            credential = Some(CredentialValue {
                kind: CredentialKind::BearerKey,
                secret: secret.trim().to_string(),
            });
        }
    }
    if credential.is_none() {
        return error_snapshot(
            AppType::Claude,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            super::missing_credential_error(
                "env 中无 ANTHROPIC_AUTH_TOKEN / ANTHROPIC_API_KEY 等任何认证字段",
            ),
        );
    }

    // 凭据形态跟随目标协议（文档 6.1：兼容字段按对应协议适配器规则发送）：
    // openai 系协议只认 Authorization: Bearer（发 x-api-key 会被 401）；gemini 用 x-goog-api-key
    if let Some(c) = &mut credential {
        c.kind = match protocol {
            ApiProtocol::OpenAiChat | ApiProtocol::OpenAiResponses => CredentialKind::BearerKey,
            ApiProtocol::GeminiNative => CredentialKind::GeminiKey,
            _ => c.kind,
        };
    }

    // ---- meta 附加项 ----
    let full_url = meta.get("isFullUrl").and_then(|v| v.as_bool()).unwrap_or(false);
    let custom_user_agent = nonempty_str(meta.get("customUserAgent")).map(|s| s.to_string());
    let overrides = meta.get("localProxyRequestOverrides");
    let headers = collect_headers(overrides.and_then(|o| o.get("headers")));

    // 用户在 overrides.headers 中显式提供了认证 Header：保留用户显式值
    let has_explicit_auth = headers.iter().any(|(k, _)| {
        let kl = k.to_lowercase();
        kl == "authorization" || kl == "x-api-key" || kl == "x-goog-api-key"
    });
    if has_explicit_auth {
        if let Some(c) = &mut credential {
            if !matches!(c.kind, CredentialKind::AuthToken | CredentialKind::ApiKeyHeader) {
                c.kind = CredentialKind::ExplicitHeader;
            }
        }
    }

    // Body 覆盖：保存规则，测试时由全局开关决定是否应用
    let local_proxy_body_patch = overrides
        .and_then(|o| o.get("body"))
        .filter(|v| v.is_object())
        .cloned();
    if local_proxy_body_patch.is_some() {
        warnings.push(
            "检测到 localProxyRequestOverrides.body（cc-switch 本地代理语义）：测试时默认按此规则改写请求体，可在测试面板关闭"
                .to_string(),
        );
    }

    ProviderSnapshot {
        app: AppType::Claude,
        provider_id: provider_id.to_string(),
        provider_name: provider_name.to_string(),
        endpoint,
        candidate_endpoints,
        protocol,
        raw_protocol: raw_protocol_str,
        models,
        credential,
        headers,
        custom_user_agent,
        client_emulation: None,
        local_proxy_body_patch,
        full_url,
        compat: serde_json::Value::Null,
        passthrough: sc.clone(),
        raw_config_hash: raw_hash,
        status: ProviderStatus::Ready,
        warnings,
        config_error: None,
    }
}

/// 加入一个模型：剥离展示标记后按 id 去重；重复时合并 markers、补齐缺失的显示名。
fn push_model(
    models: &mut Vec<crate::domain::ModelSnapshot>,
    index: &mut std::collections::HashMap<String, usize>,
    raw: &str,
    display_name: Option<&str>,
    one_m: bool,
) {
    let (id, mut markers) = crate::domain::strip_model_id_markers(raw);
    if id.is_empty() {
        return;
    }
    if one_m && !markers.iter().any(|x| x.eq_ignore_ascii_case("1M")) {
        markers.push("1M".to_string());
    }
    let display_name = display_name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    match index.get(&id) {
        Some(&i) => {
            let m = &mut models[i];
            for mk in markers {
                if !m.id_markers.iter().any(|x| x.eq_ignore_ascii_case(&mk)) {
                    m.id_markers.push(mk);
                }
            }
            if m.display_name.is_none() {
                m.display_name = display_name;
            }
        }
        None => {
            index.insert(id.clone(), models.len());
            let mut snap = super::model_with_markers(id, markers);
            snap.display_name = display_name;
            models.push(snap);
        }
    }
}

/// 托管认证跳过快照。
fn managed_snapshot(
    provider_id: &str,
    provider_name: &str,
    raw_hash: String,
    candidate_endpoints: Vec<String>,
) -> ProviderSnapshot {
    let mut snap = error_snapshot(
        AppType::Claude,
        provider_id,
        provider_name,
        raw_hash,
        candidate_endpoints,
        String::new(),
    );
    snap.status = ProviderStatus::ManagedAuthSkipped;
    snap.config_error = None;
    snap
}
