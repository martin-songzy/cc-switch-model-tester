//! URL 规范化与端点路径（DevelopmentPlan.md 7.2）。

use crate::domain::{ApiProtocol, TestMode};

/// 规范化 Base URL：
/// - 去除首尾空白
/// - scheme 限 http/https
/// - host 小写
/// - 默认端口归一（https:443 / http:80 去除）
/// - 去除路径末尾多余 `/`
/// - 保留路径与 query
pub fn normalize_base(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("URL 为空".to_string());
    }
    let (scheme, rest) = s
        .split_once("://")
        .ok_or_else(|| format!("URL 缺少协议前缀（需 http/https）: {s}"))?;
    let scheme_l = scheme.to_lowercase();
    if scheme_l != "http" && scheme_l != "https" {
        return Err(format!("不支持的协议 \"{scheme}\"（仅 http/https）"));
    }
    // rest = host[:port][/path][?query]
    let (authority_and_path, query) = match rest.split_once('?') {
        Some((a, q)) => (a, Some(q)),
        None => (rest, None),
    };
    let mut parts = authority_and_path.splitn(2, '/');
    let host_port = parts.next().unwrap_or_default().to_lowercase();
    let path = parts.next().unwrap_or("");
    if host_port.is_empty() {
        return Err(format!("URL 缺少主机名: {s}"));
    }
    let authority = normalize_host_port(&host_port, &scheme_l);
    let path = path.trim_end_matches('/').to_string();
    let path_seg = if path.is_empty() {
        String::new()
    } else {
        format!("/{path}")
    };
    let mut out = format!("{scheme_l}://{authority}{path_seg}");
    if let Some(q) = query {
        if !q.is_empty() {
            out.push('?');
            out.push_str(q);
        }
    }
    Ok(out)
}

fn normalize_host_port(host_port: &str, scheme: &str) -> String {
    // 仅处理常见 host:port 形式（IPv6 包含冒号且可能带 []，保守处理）
    match host_port.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') || host_port.ends_with(']') && !h.ends_with('[') => {
            match (scheme, p) {
                ("https", "443") | ("http", "80") => h.to_string(),
                _ => host_port.to_string(),
            }
        }
        _ => host_port.to_string(),
    }
}

/// 路径段编码：保留 RFC3986 unreserved 与 `:`（Google 约定 model:action 形式）。
pub fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn base_ends_with_segment(base_path: &str, segment: &str) -> bool {
    let base_path = base_path.trim_end_matches('/');
    base_path == segment || base_path.ends_with(&format!("/{segment}"))
}

/// 拆出 base 的 path 与 query，便于在 path 后追加端点、保留 query。
fn split_path_query(normalized_base: &str) -> (String, Option<String>) {
    match normalized_base.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (normalized_base.to_string(), None),
    }
}

fn join_with_query(mut url: String, query: Option<&str>) -> String {
    if let Some(q) = query {
        if !q.is_empty() {
            url.push('?');
            url.push_str(q);
        }
    }
    url
}

/// 构造完整请求 URL。
/// full_url = true 时把配置地址当完整请求地址（仍做 scheme 校验）。
pub fn build_endpoint_url(
    target: &crate::domain::TestTarget,
    mode: TestMode,
) -> Result<String, String> {
    let base = normalize_base(&target.endpoint_url)?;
    if target.full_url {
        return Ok(base);
    }
    let (path_base, query) = split_path_query(&base);
    // path_base = scheme://host[:port][/path]
    let (scheme, authority, base_path) = {
        let (sch, after_scheme) = path_base
            .split_once("://")
            .ok_or_else(|| "内部错误：规范化 URL 缺少 scheme".to_string())?;
        match after_scheme.split_once('/') {
            Some((h, p)) => (sch.to_string(), h.to_string(), format!("/{p}")),
            None => (sch.to_string(), after_scheme.to_string(), String::new()),
        }
    };

    let model = &target.model_id;
    let suffix: String = match target.protocol {
        ApiProtocol::AnthropicMessages => {
            // {base}/v1/messages；base 已以 /v1 结尾则不重复追加
            if base_ends_with_segment(&base_path, "v1") {
                "/messages".to_string()
            } else {
                "/v1/messages".to_string()
            }
        }
        ApiProtocol::OpenAiChat => "/chat/completions".to_string(),
        ApiProtocol::OpenAiResponses => "/responses".to_string(),
        ApiProtocol::GeminiNative => {
            let m = encode_path_segment(model);
            let action = match mode {
                TestMode::NonStreaming => "generateContent",
                TestMode::Streaming => "streamGenerateContent",
            };
            if base_ends_with_segment(&base_path, "v1beta") {
                format!("/models/{m}:{action}")
            } else {
                format!("/v1beta/models/{m}:{action}")
            }
        }
        ApiProtocol::BedrockConverseStream => {
            return Err("Bedrock 第一版不发送请求".to_string())
        }
    };

    Ok(join_with_query(
        format!("{scheme}://{authority}{base_path}{suffix}"),
        query.as_deref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_base() {
        assert_eq!(normalize_base("https://API.Example.com/").unwrap(), "https://api.example.com");
        assert_eq!(normalize_base(" https://x.com/v1/ ").unwrap(), "https://x.com/v1");
        assert_eq!(
            normalize_base("https://x.com:443/v1").unwrap(),
            "https://x.com/v1"
        );
        assert_eq!(normalize_base("http://x.com:8080/api/").unwrap(), "http://x.com:8080/api");
        assert_eq!(normalize_base("https://x.com/a?k=1").unwrap(), "https://x.com/a?k=1");
        assert!(normalize_base("ftp://x.com").is_err());
        assert!(normalize_base("nonsense").is_err());
        assert!(normalize_base("  ").is_err());
    }

    #[test]
    fn encodes_path_segment() {
        assert_eq!(encode_path_segment("gemini-2.5-pro"), "gemini-2.5-pro");
        assert_eq!(encode_path_segment("a b/c"), "a%20b%2Fc");
        assert_eq!(encode_path_segment("中文名"), "%E4%B8%AD%E6%96%87%E5%90%8D");
    }
}
