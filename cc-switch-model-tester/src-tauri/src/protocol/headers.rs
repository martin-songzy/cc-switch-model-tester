//! 请求 Header 组装（DevelopmentPlan.md 7.3）。
//!
//! 优先级：
//! 1. HTTP 客户端默认（reqwest 层）
//! 2. 协议固定 Header（content-type / anthropic-version / User-Agent 基准值）
//! 3. 供应商字面量自定义 Header（同名覆盖协议固定值）
//! 4. 认证 Header 生成（若字面量已含同名认证 Header 则保留用户显式值）
//! 5. `meta.customUserAgent` 最后覆盖所有 User-Agent
//!
//! 保护 Header（host/content-length/transfer-encoding/connection/proxy-*、te/trailer/
//! upgrade/accept-encoding）不允许出现在用户配置中，遇到即丢弃并记录。

use crate::domain::{ApiProtocol, CredentialKind, TestTarget};

/// 用户配置中禁止出现的 Header（大小写不敏感）。
pub const PROTECTED_HEADERS: [&str; 11] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "proxy-authorization",
    "proxy-authenticate",
    "te",
    "trailer",
    "upgrade",
    "accept-encoding",
    "chatgpt-account-id",
];

const AUTH_HEADER_NAMES: [&str; 3] = ["authorization", "x-api-key", "x-goog-api-key"];

fn is_protected(name: &str) -> bool {
    let n = name.to_lowercase();
    PROTECTED_HEADERS.iter().any(|p| *p == n)
}

fn has_control_chars(s: &str) -> bool {
    s.chars().any(|c| c.is_control())
}

pub struct BuiltHeaders {
    pub headers: Vec<(String, String)>,
    /// 被丢弃的保护 Header（名称），用于警告
    pub dropped_protected: Vec<String>,
    /// 因含控制字符被丢弃的 Header
    pub dropped_invalid: Vec<String>,
}

/// 将 beta flag 合并进 anthropic-beta 头（逗号分隔，去重；无该头则新增）。
pub fn merge_beta_header(flag: &str, out: &mut Vec<(String, String)>) {
    const KEY: &str = "anthropic-beta";
    let flag_trim = flag.trim();
    if flag_trim.is_empty() {
        return;
    }
    if let Some(existing) = out.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(KEY)) {
        let mut flags: Vec<String> = existing
            .1
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !flags.iter().any(|f| f.eq_ignore_ascii_case(flag_trim)) {
            flags.push(flag_trim.to_string());
        }
        existing.1 = flags.join(",");
    } else {
        out.push((KEY.to_string(), flag_trim.to_string()));
    }
}

/// 组装最终请求 Header。
pub fn build_request_headers(
    target: &TestTarget,
    app_user_agent: &str,
    anthropic_version: &str,
) -> BuiltHeaders {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut dropped_protected = Vec::new();
    let mut dropped_invalid = Vec::new();

    // 1. 协议固定 Header
    let push = |name: &str, value: String, out: &mut Vec<(String, String)>| {
        if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case(name)) {
            out.push((name.to_string(), value));
        }
    };
    push("content-type", "application/json".to_string(), &mut out);
    push("user-agent", app_user_agent.to_string(), &mut out);
    if target.protocol == ApiProtocol::AnthropicMessages {
        push("anthropic-version", anthropic_version.to_string(), &mut out);
        // cc-switch 模型标记 [1M] → 启用 1M 上下文 beta（与仿真等其他 beta 逗号合并）
        if target.id_markers.iter().any(|m| m.eq_ignore_ascii_case("1M")) {
            merge_beta_header("context-1m-2025-08-07", &mut out);
        }
    }

    // 2. 供应商字面量 Header（过滤保护项与非法项；同名覆盖协议固定值）
    for (name, value) in &target.headers {
        if is_protected(name) {
            dropped_protected.push(name.clone());
            continue;
        }
        if has_control_chars(name) || has_control_chars(value) || name.is_empty() {
            dropped_invalid.push(name.clone());
            continue;
        }
        // 同名（大小写不敏感）覆盖
        if let Some(existing) = out.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
            existing.1 = value.clone();
        } else {
            out.push((name.clone(), value.clone()));
        }
    }

    // 3. 认证 Header 生成（字面量已含同名认证 Header 则保留用户显式值，不生成）
    let has_explicit_auth = out
        .iter()
        .any(|(k, _)| AUTH_HEADER_NAMES.iter().any(|a| k.eq_ignore_ascii_case(a)));
    if !has_explicit_auth {
        if let Some(cred) = &target.credential {
            let auth = match cred.kind {
                CredentialKind::AuthToken | CredentialKind::BearerKey => {
                    Some(("authorization", format!("Bearer {}", cred.secret)))
                }
                CredentialKind::ApiKeyHeader => Some(("x-api-key", cred.secret.clone())),
                CredentialKind::GeminiKey => Some(("x-goog-api-key", cred.secret.clone())),
                // ExplicitHeader 不生成（值已在 headers 中）；其余不生成
                _ => None,
            };
            if let Some((name, value)) = auth {
                push(name, value, &mut out);
            }
        }
    }

    // 4. customUserAgent 最后覆盖
    if let Some(ua) = &target.custom_user_agent {
        if !ua.is_empty() && !has_control_chars(ua) {
            if let Some(existing) = out.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("user-agent")) {
                existing.1 = ua.clone();
            }
        }
    }

    BuiltHeaders {
        headers: out,
        dropped_protected,
        dropped_invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AppType, CredentialValue};

    fn target(protocol: ApiProtocol, headers: Vec<(String, String)>, cred: Option<CredentialKind>) -> TestTarget {
        TestTarget {
            app: AppType::Claude,
            provider_id: "p".into(),
            provider_name: "P".into(),
            model_id: "m".into(),
            model_display_name: None,
            endpoint_url: "https://x.example".into(),
            protocol,
            credential: cred.map(|kind| CredentialValue { kind, secret: "sk-test".into() }),
            headers,
            custom_user_agent: None,
            emulation: false,
            emulation_profile: None,
            client_emulation: None,
            local_proxy_body_patch: None,
            id_markers: vec![],
            full_url: false,
            compat: serde_json::Value::Null,
        }
    }

    #[test]
    fn anthropic_defaults_and_credential() {
        let t = target(ApiProtocol::AnthropicMessages, vec![], Some(CredentialKind::AuthToken));
        let b = build_request_headers(&t, "tester/0.1", "2023-06-01");
        assert!(b.headers.iter().any(|(k, v)| k == "content-type" && v == "application/json"));
        assert!(b.headers.iter().any(|(k, v)| k == "anthropic-version" && v == "2023-06-01"));
        assert!(b.headers.iter().any(|(k, v)| k == "authorization" && v == "Bearer sk-test"));
        assert!(!b.headers.iter().any(|(k, _)| k == "x-api-key"));
    }

    #[test]
    fn one_m_marker_adds_context_beta() {
        let mut t = target(ApiProtocol::AnthropicMessages, vec![], Some(CredentialKind::AuthToken));
        t.id_markers = vec!["1M".into()];
        let b = build_request_headers(&t, "tester/0.1", "2023-06-01");
        assert!(b.headers.iter().any(
            |(k, v)| k == "anthropic-beta" && v.contains("context-1m-2025-08-07")
        ));
        // 无标记 → 无 beta 头
        let b2 = build_request_headers(&target(ApiProtocol::AnthropicMessages, vec![], None), "tester/0.1", "2023-06-01");
        assert!(!b2.headers.iter().any(|(k, _)| k == "anthropic-beta"));
    }

    #[test]
    fn beta_merges_between_build_and_emulation() {
        use crate::emulation;
        let mut t = target(ApiProtocol::AnthropicMessages, vec![], Some(CredentialKind::AuthToken));
        t.id_markers = vec!["1M".into()];
        t.emulation = true;
        t.emulation_profile = Some("claude-code".into());
        let mut req = crate::protocol::build_request(&t, "hi", crate::domain::TestMode::NonStreaming, 64).unwrap();
        let profile = emulation::resolve_profile(&t).unwrap();
        emulation::apply_client_emulation(&mut req, &t, profile, "run-x");
        let beta = req
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("anthropic-beta"))
            .map(|(_, v)| v.clone())
            .unwrap();
        assert!(beta.contains("claude-code-20250219"), "画像 beta 应存在");
        assert!(beta.contains("context-1m-2025-08-07"), "1M beta 不应被画像覆盖");
    }

    #[test]
    fn user_literal_overrides_protocol_default_and_auth() {
        let t = target(
            ApiProtocol::AnthropicMessages,
            vec![
                ("anthropic-version".into(), "2099-01-01".into()),
                ("authorization".into(), "Bearer user-key".into()),
            ],
            Some(CredentialKind::AuthToken),
        );
        let b = build_request_headers(&t, "tester/0.1", "2023-06-01");
        // 用户显式 anthropic-version 保留
        assert!(b.headers.iter().any(|(k, v)| k == "anthropic-version" && v == "2099-01-01"));
        // 用户显式 Authorization 保留，不生成第二个
        assert_eq!(
            b.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("authorization")).count(),
            1
        );
        assert!(b.headers.iter().any(|(k, v)| k == "authorization" && v == "Bearer user-key"));
    }

    #[test]
    fn protected_headers_dropped() {
        let t = target(
            ApiProtocol::OpenAiChat,
            vec![("Host".into(), "evil.example".into()), ("content-length".into(), "1".into())],
            Some(CredentialKind::BearerKey),
        );
        let b = build_request_headers(&t, "tester/0.1", "2023-06-01");
        assert!(b.dropped_protected.contains(&"Host".to_string()));
        assert!(!b.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")));
    }

    #[test]
    fn custom_user_agent_wins_last() {
        let mut t = target(ApiProtocol::OpenAiChat, vec![], Some(CredentialKind::BearerKey));
        t.custom_user_agent = Some("claude-cli/9.9".into());
        let b = build_request_headers(&t, "tester/0.1", "2023-06-01");
        assert!(b.headers.iter().any(|(k, v)| k == "user-agent" && v == "claude-cli/9.9"));
    }

    #[test]
    fn control_chars_rejected() {
        let t = target(
            ApiProtocol::OpenAiChat,
            vec![("X-Bad".into(), "v1\r\ninjected: yes".into())],
            None,
        );
        let b = build_request_headers(&t, "tester/0.1", "2023-06-01");
        assert!(b.dropped_invalid.contains(&"X-Bad".to_string()));
        assert!(!b.headers.iter().any(|(k, _)| k == "X-Bad"));
    }
}
