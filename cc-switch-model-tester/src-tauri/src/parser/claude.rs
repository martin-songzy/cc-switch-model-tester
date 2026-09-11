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
//!   "localProxyRequestOverrides": { "headers": { }, "body": { } } }
//! ```

use serde_json::Value;

use crate::domain::{
    ApiProtocol, AppType, CredentialKind, CredentialValue, ProviderSnapshot, ProviderStatus,
};

use super::{collect_headers, detect_managed, error_snapshot, nonempty_str};

const MODEL_ENV_KEYS: [&str; 6] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
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

    // ---- 模型集合：全部模型字段加入后去重，剥离 [1M] 类展示标记 ----
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(env) = env {
        for key in MODEL_ENV_KEYS {
            if let Some(raw) = nonempty_str(env.get(key)) {
                let (id, markers) = crate::domain::strip_model_id_markers(raw);
                if id.is_empty() || !seen.insert(id.clone()) {
                    continue;
                }
                models.push(super::model_with_markers(id, markers));
            }
        }
    }
    if models.is_empty() {
        return error_snapshot(
            AppType::Claude,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            "模型列表为空（env 中未找到任何模型字段）".to_string(),
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

    // Body 覆盖：默认不应用（文档 7.4），仅提示
    if overrides.and_then(|o| o.get("body")).is_some() {
        warnings.push(
            "检测到 localProxyRequestOverrides.body（cc-switch 本地代理语义），默认不应用于直连测试"
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
        full_url,
        compat: serde_json::Value::Null,
        passthrough: sc.clone(),
        raw_config_hash: raw_hash,
        status: ProviderStatus::Ready,
        warnings,
        config_error: None,
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
