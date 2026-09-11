//! 脱敏工具。
//!
//! 任何要写入日志、发送到前端或写入历史库的文本 / URL，
//! 都必须先经过本模块处理，确保 API Key、Auth Token 不泄露。
//! 参见 DevelopmentPlan.md 第 18 节。

use std::sync::LazyLock;
use regex::Regex;

/// `Authorization: Bearer xxx` / `Bearer xxx` 中的凭据
static RE_BEARER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9\-._~+/]+=*").unwrap()
});

/// `sk-` 前缀密钥（OpenAI / 部分中转站格式）
static RE_SK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-[A-Za-z0-9_\-]{8,}").unwrap());

/// JSON 字段级敏感键：`"api_key": "xxx"` 等
static RE_JSON_SENSITIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)("(?:api[_-]?key|apikey|access[_-]?token|auth[_-]?token|token|secret|authorization|password|refresh[_-]?token)"\s*:\s*")([^"]*)(")"#)
        .unwrap()
});

/// Header 行格式敏感项：`x-api-key: xxx`（值到行尾或逗号）
static RE_HEADER_SENSITIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?im)^((?:x-)?(?:api-key|apikey|authorization|auth|proxy-authorization|secret|token))(\s*:\s*)([^\r\n,]+)"#)
        .unwrap()
});

/// 对任意文本执行常见密钥格式脱敏。
pub fn redact_text(input: &str) -> String {
    let s = RE_BEARER.replace_all(input, "${1}***");
    let s = RE_SK.replace_all(&s, "sk-***");
    let s = RE_JSON_SENSITIVE.replace_all(&s, "${1}***${3}");
    let s = RE_HEADER_SENSITIVE.replace_all(&s, "${1}${2}***");
    s.into_owned()
}

/// 凭据提示：只暴露末尾 4 位（用于区分用的是哪个 key），绝不返回明文。
/// 空串返回 None；过短（≤8 字符）只返回 ***。
pub fn credential_hint(secret: &str) -> Option<String> {
    let s = secret.trim();
    if s.is_empty() {
        return None;
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 8 {
        Some("***".to_string())
    } else {
        let tail: String = chars[chars.len() - 4..].iter().collect();
        Some(format!("…{tail}"))
    }
}

/// 对 URL 执行脱敏：查询参数的值全部替换为 ***，保留键名与路径。
pub fn redact_url(input: &str) -> String {
    match input.split_once('?') {
        None => input.to_string(),
        Some((base, query)) => {
            let redacted: Vec<String> = query
                .split('&')
                .filter(|p| !p.is_empty())
                .map(|p| match p.split_once('=') {
                    Some((k, _)) => format!("{k}=***"),
                    None => format!("{p}=***"),
                })
                .collect();
            if redacted.is_empty() {
                base.to_string()
            } else {
                format!("{}?{}", base, redacted.join("&"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_hint_shows_tail_only() {
        assert_eq!(credential_hint("sk-ant-api03-abcdefgh"), Some("…efgh".to_string()));
        assert_eq!(credential_hint("short"), Some("***".to_string()));
        assert_eq!(credential_hint("12345678"), Some("***".to_string()));
        assert_eq!(credential_hint("   "), None);
        assert_eq!(credential_hint(""), None);
    }

    #[test]
    fn redacts_bearer_tokens() {
        // 独立的 Bearer 凭据（前缀脱敏）
        assert_eq!(redact_text("Bearer abc123XYZ.def"), "Bearer ***");
        // 标准 Header 名 + 值：整个值脱敏（Header 规则更严格，符合安全预期）
        assert_eq!(
            redact_text("Authorization: Bearer abc123XYZ.def"),
            "Authorization: ***"
        );
    }

    #[test]
    fn redacts_sk_keys() {
        assert_eq!(
            redact_text("key=sk-abcdefgh12345678 剩余文本"),
            "key=sk-*** 剩余文本"
        );
    }

    #[test]
    fn redacts_json_sensitive_fields() {
        assert_eq!(
            redact_text(r#"{"apiKey":"sk-secret123","name":"x"}"#),
            r#"{"apiKey":"***","name":"x"}"#
        );
    }

    #[test]
    fn redacts_header_lines() {
        assert_eq!(
            redact_text("x-api-key: abcdef12345\naccept: json"),
            "x-api-key: ***\naccept: json"
        );
    }

    #[test]
    fn redacts_url_query_values() {
        assert_eq!(
            redact_url("https://example.com/v1/messages?key=sk-abc&page=2"),
            "https://example.com/v1/messages?key=***&page=***"
        );
    }

    #[test]
    fn keeps_url_without_query() {
        assert_eq!(redact_url("https://example.com/v1"), "https://example.com/v1");
    }

    #[test]
    fn normal_text_untouched() {
        assert_eq!(redact_text("普通文本 Result 和 Option"), "普通文本 Result 和 Option");
    }
}
