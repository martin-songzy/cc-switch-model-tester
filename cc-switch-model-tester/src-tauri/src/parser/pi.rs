//! Pi agent 供应商解析（DevelopmentPlan.md 6.3）。
//!
//! settings_config 形如（与 Pi 原生 models.json 的 provider 节点同形）：
//! ```json
//! { "name": "...", "baseUrl": "https://...", "api": "anthropic-messages",
//!   "apiKey": "...", "headers": { }, "compat": { },
//!   "models": [ { "id": "...", "name": "...", "baseUrl": null, "compat": { },
//!                "thinkingLevelMap": { } } ] }
//! ```
//! cc-switch 将该配置作为不透明 JSON 透传（pi_config 只校验形状），
//! 因此未知字段一律保留到 passthrough，不猜测含义。

use serde_json::Value;

use crate::domain::{
    ApiProtocol, AppType, ClientEmulationSpec, CredentialKind, CredentialValue, ModelSnapshot,
    ProviderSnapshot, ProviderStatus,
};

use super::{collect_headers, detect_managed, error_snapshot, nonempty_str};

pub(super) fn parse(
    provider_id: &str,
    provider_name: &str,
    sc: &Value,
    meta: &Value,
    candidate_endpoints: Vec<String>,
    raw_hash: String,
) -> ProviderSnapshot {
    let mut warnings: Vec<String> = Vec::new();

    // ---- Base URL ----
    let endpoint = match nonempty_str(sc.get("baseUrl")) {
        Some(u) => u.trim().to_string(),
        None => {
            return error_snapshot(
                AppType::Pi,
                provider_id,
                provider_name,
                raw_hash,
                candidate_endpoints,
                "缺少 Base URL（settings_config.baseUrl 为空）".to_string(),
            )
        }
    };

    if detect_managed(meta, Some(&endpoint)) {
        let mut snap = error_snapshot(
            AppType::Pi,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            String::new(),
        );
        snap.status = ProviderStatus::ManagedAuthSkipped;
        snap.config_error = None;
        return snap;
    }

    // ---- 协议映射 ----
    let raw_api = nonempty_str(sc.get("api"));
    let protocol = match raw_api {
        Some("openai-completions") => ApiProtocol::OpenAiChat,
        Some("openai-responses") => ApiProtocol::OpenAiResponses,
        Some("anthropic-messages") => ApiProtocol::AnthropicMessages,
        Some("google-generative-ai") => ApiProtocol::GeminiNative,
        Some("bedrock-converse-stream") => {
            // 第一版仅识别、不执行（文档 8.5），保留模型供展示
            let models = collect_pi_models(sc);
            let mut snap = error_snapshot(
                AppType::Pi,
                provider_id,
                provider_name,
                raw_hash,
                candidate_endpoints,
                String::new(),
            );
            snap.status = ProviderStatus::UnsupportedProtocol;
            snap.config_error = None;
            snap.endpoint = endpoint;
            snap.protocol = ApiProtocol::BedrockConverseStream;
            snap.raw_protocol = raw_api.map(|s| s.to_string());
            snap.models = models;
            snap.passthrough = sc.clone();
            return snap;
        }
        Some(other) => {
            return error_snapshot(
                AppType::Pi,
                provider_id,
                provider_name,
                raw_hash,
                candidate_endpoints,
                format!(
                    "未知的 Pi 协议值 \"{other}\"（合法值: openai-completions/openai-responses/anthropic-messages/google-generative-ai/bedrock-converse-stream）"
                ),
            )
        }
        None => {
            return error_snapshot(
                AppType::Pi,
                provider_id,
                provider_name,
                raw_hash,
                candidate_endpoints,
                "缺少协议字段 settings_config.api".to_string(),
            )
        }
    };

    // ---- 模型 ----
    let models = collect_pi_models(sc);
    if models.is_empty() {
        return error_snapshot(
            AppType::Pi,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            "模型列表为空（settings_config.models 缺失或为空）".to_string(),
        );
    }

    // ---- 供应商级 compat（对象）----
    let provider_compat = sc.get("compat").cloned().unwrap_or(Value::Null);

    // ---- 凭据：headers 显式认证优先；否则 apiKey，发送形态由 authHeader/协议决定 ----
    // Pi 官方语义（models.md）：authHeader=true → 自动添加 Authorization: Bearer <apiKey>；
    // 否则按协议默认（anthropic-messages → x-api-key，gemini → x-goog-api-key，openai → Bearer）
    let headers = collect_headers(sc.get("headers"));
    let has_explicit_auth = headers.iter().any(|(k, _)| {
        let kl = k.to_lowercase();
        kl == "authorization" || kl == "x-api-key" || kl == "x-goog-api-key"
    });

    let auth_header_flag = sc.get("authHeader");
    if let Some(v) = auth_header_flag {
        if !v.is_boolean() {
            warnings.push(
                "authHeader 字段应为布尔值（Pi 原生语义），已忽略异常取值".to_string(),
            );
        }
    }
    let use_bearer = auth_header_flag.and_then(|v| v.as_bool()).unwrap_or(false);

    let credential: Option<CredentialValue> = if has_explicit_auth {
        Some(CredentialValue {
            kind: CredentialKind::ExplicitHeader,
            secret: String::new(), // 值在 headers 中，M3 直接使用
        })
    } else if let Some(key) = nonempty_str(sc.get("apiKey")) {
        let kind = if use_bearer {
            CredentialKind::AuthToken
        } else {
            match protocol {
                ApiProtocol::AnthropicMessages => CredentialKind::ApiKeyHeader,
                ApiProtocol::GeminiNative => CredentialKind::GeminiKey,
                _ => CredentialKind::BearerKey,
            }
        };
        Some(CredentialValue {
            kind,
            secret: key.trim().to_string(),
        })
    } else {
        None
    };

    if credential.is_none() {
        return error_snapshot(
            AppType::Pi,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            super::missing_credential_error("无 headers.Authorization / authHeader / apiKey"),
        );
    }

    // ---- clientEmulation：客户端仿真配置（每模型开关在目录页控制，默认值取 enabled）
    let client_emulation = sc.get("clientEmulation").and_then(|v| v.as_object()).map(|ce| {
        let headers = ce
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        (k.to_lowercase(), v.as_str().map(|s| s.to_string()))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        ClientEmulationSpec {
            enabled: ce.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
            profile: nonempty_str(ce.get("profile")).unwrap_or("claude-code").to_string(),
            user_agent: nonempty_str(ce.get("userAgent")).map(|s| s.to_string()),
            headers,
        }
    });
    if client_emulation.is_some() {
        warnings.push(
            "检测到 clientEmulation 配置：可在模型行开关客户端仿真（默认跟随此配置）"
                .to_string(),
        );
    }

    ProviderSnapshot {
        app: AppType::Pi,
        provider_id: provider_id.to_string(),
        provider_name: provider_name.to_string(),
        endpoint,
        candidate_endpoints,
        protocol,
        raw_protocol: raw_api.map(|s| s.to_string()),
        models,
        credential,
        headers,
        custom_user_agent: nonempty_str(meta.get("customUserAgent")).map(|s| s.to_string()),
        client_emulation,
        local_proxy_body_patch: meta
            .get("localProxyRequestOverrides")
            .and_then(|o| o.get("body"))
            .filter(|v| v.is_object())
            .cloned(),
        full_url: meta.get("isFullUrl").and_then(|v| v.as_bool()).unwrap_or(false),
        compat: provider_compat,
        passthrough: sc.clone(),
        raw_config_hash: raw_hash,
        status: ProviderStatus::Ready,
        warnings,
        config_error: None,
    }
}

/// 收集 models[]：id 必填；模型级 baseUrl 覆盖供应商级；compat 深度合并（模型级优先）；
/// thinkingLevelMap 原样保留；未知字段随 passthrough 保留在 sc 原文中。
fn collect_pi_models(sc: &Value) -> Vec<ModelSnapshot> {
    let Some(arr) = sc.get("models").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let provider_compat = sc.get("compat").cloned().unwrap_or(Value::Null);

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for m in arr {
        let Some(raw_id) = nonempty_str(m.get("id")) else {
            continue;
        };
        let (id, markers) = crate::domain::strip_model_id_markers(raw_id);
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        // compat 合并：对象时模型级字段优先（浅合并顶层键，符合“对象合并，模型级优先”）
        let model_compat = m.get("compat").cloned().unwrap_or(Value::Null);
        let merged = merge_compat(&provider_compat, &model_compat);

        out.push(ModelSnapshot {
            model_id: id,
            display_name: nonempty_str(m.get("name")).map(|s| s.to_string()),
            base_url_override: nonempty_str(m.get("baseUrl")).map(|s| s.to_string()),
            compat: merged,
            thinking_level_map: m.get("thinkingLevelMap").cloned(),
            id_markers: markers,
        });
    }
    out
}

/// compat 对象合并：模型级优先（浅合并顶层键）。
fn merge_compat(provider: &Value, model: &Value) -> Value {
    match (provider.as_object(), model.as_object()) {
        (Some(p), Some(m)) => {
            let mut merged = p.clone();
            for (k, v) in m {
                merged.insert(k.clone(), v.clone());
            }
            Value::Object(merged)
        }
        (_, Some(m)) if !m.is_empty() => Value::Object(m.clone()),
        (Some(p), _) => Value::Object(p.clone()),
        _ => Value::Null,
    }
}
