//! HTTP 执行层：共享 reqwest Client + 代理 + 超时 + 流式读取（文档 7.1 / 8.6 / 17）。
//!
//! 原则：
//! - 代理配置生效时所有请求都走代理，失败不回退直连（文档 2.1）
//! - 失败不自动重试（文档 10.3）
//! - 响应字节上限 8 MiB、可提取文本上限 1 Mi（文档 8.6）

use std::time::{Duration, Instant};

use futures_util::StreamExt;

use crate::protocol::{
    ParsedResponse, PreparedRequest, ProtocolAdapter, TestMode,
    MAX_RESPONSE_BYTES, MAX_TEXT_CHARS,
};

#[derive(Debug, Clone)]
pub struct RequestLimits {
    pub connect_timeout: Duration,
    pub total_timeout: Duration,
    pub idle_timeout: Duration,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            total_timeout: Duration::from_secs(180),
            idle_timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOutput {
    pub http_status: Option<u16>,
    pub parsed: ParsedResponse,
    pub first_byte_ms: Option<u64>,
    pub total_ms: u64,
    pub response_bytes: u64,
    pub response_truncated: bool,
    pub text_truncated: bool,
    /// 网络层错误（连接/DNS/TLS/超时/代理），错误消息不得含密钥
    pub network_error: Option<String>,
}

pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    /// 创建共享客户端。`proxy` 为 None 时直连；Some 时代理失败不回退。
    pub fn new(proxy: Option<&str>, limits: &RequestLimits) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(limits.connect_timeout)
            .user_agent(crate::APP_USER_AGENT);
        if let Some(p) = proxy {
            let url = p.trim();
            if !url.is_empty() {
                let proxy = reqwest::Proxy::all(url)
                    .map_err(|e| format!("代理配置无效: {e}"))?;
                builder = builder.proxy(proxy);
            }
        }
        let client = builder.build().map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;
        Ok(Self { client })
    }

    /// 执行一次测试请求并解析响应。
    pub async fn execute(
        &self,
        prepared: &PreparedRequest,
        adapter: &dyn ProtocolAdapter,
        mode: TestMode,
        limits: &RequestLimits,
    ) -> ExecutionOutput {
        let start = Instant::now();
        let mut out = ExecutionOutput::default();

        let mut request = self.client.post(&prepared.url);
        for (k, v) in &prepared.headers {
            // 保护 Header 已在组装层过滤；此处跳过 reqwest 禁止手动设置的项
            let kl = k.to_lowercase();
            if kl == "host" || kl == "content-length" {
                continue;
            }
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
                if let Ok(val) = reqwest::header::HeaderValue::from_str(v) {
                    request = request.header(name, val);
                }
            }
        }
        let body = serde_json::to_string(&prepared.body).unwrap_or_default();
        request = request.body(body);

        let deadline = tokio::time::sleep(limits.total_timeout);
        tokio::pin!(deadline);

        let send_result = tokio::select! {
            r = request.send() => r,
            _ = &mut deadline => {
                out.total_ms = start.elapsed().as_millis() as u64;
                out.network_error = Some("请求总超时".to_string());
                return out;
            }
        };

        let response = match send_result {
            Ok(r) => r,
            Err(e) => {
                out.total_ms = start.elapsed().as_millis() as u64;
                out.network_error = Some(classify_reqwest_error(&e));
                return out;
            }
        };

        out.first_byte_ms = Some(start.elapsed().as_millis() as u64);
        out.http_status = Some(response.status().as_u16());

        let mut stream = response.bytes_stream();
        let mut first_text_recorded = false;

        if mode == TestMode::NonStreaming {
            // 非流式：读完整个 body（限 8 MiB）后交给适配器
            let mut buf: Vec<u8> = Vec::new();
            loop {
                tokio::select! {
                    chunk = tokio::time::timeout(limits.idle_timeout, stream.next()) => {
                        match chunk {
                            Err(_) => {
                                out.parsed.parse_error = Some("响应读取超时".to_string());
                                break;
                            }
                            Ok(None) => break,
                            Ok(Some(Ok(bytes))) => {
                                out.response_bytes += bytes.len() as u64;
                                if out.response_bytes > MAX_RESPONSE_BYTES {
                                    out.response_truncated = true;
                                    out.parsed.aborted = true;
                                    out.parsed.parse_error = Some(
                                        "响应超出 8 MiB 上限，已中止读取".to_string(),
                                    );
                                    break;
                                }
                                buf.extend_from_slice(&bytes);
                            }
                            Ok(Some(Err(e))) => {
                                out.network_error = Some(classify_reqwest_error(&e));
                                break;
                            }
                        }
                    }
                    _ = &mut deadline => {
                        out.network_error = Some("请求总超时".to_string());
                        break;
                    }
                }
            }
            if !out.parsed.aborted {
                out.parsed = adapter.parse_non_stream(&buf);
                // 解析失败时保留响应原文开头（脱敏），替代无信息量的“JSON 解析失败”
                if out.parsed.parse_error.is_some() || out.parsed.error_object.is_some() {
                    let head: String = String::from_utf8_lossy(&buf).chars().take(300).collect();
                    out.parsed.raw_body_head = Some(redact_head(&head));
                }
            }
        } else {
            // 流式：逐块喂给解析器，带空闲超时
            let mut parser = adapter.new_stream_parser();
            loop {
                tokio::select! {
                    chunk = tokio::time::timeout(limits.idle_timeout, stream.next()) => {
                        match chunk {
                            Err(_) => {
                                out.parsed.parse_error = Some("流式空闲超时".to_string());
                                break;
                            }
                            Ok(None) => break, // EOF → finish 由 parser 判定
                            Ok(Some(Ok(bytes))) => {
                                parser.feed_chunk(&bytes);
                                if parser.text_len() > MAX_TEXT_CHARS {
                                    out.text_truncated = true;
                                    out.parsed.aborted = true;
                                    out.parsed.parse_error = Some(
                                        "累计文本超出 1 Mi 上限，已中止".to_string(),
                                    );
                                    break;
                                }
                                if !first_text_recorded && parser.text_len() > 0 {
                                    first_text_recorded = true;
                                    // first_text_ms 记录到 parsed 之后统一处理（M4）
                                }
                            }
                            Ok(Some(Err(e))) => {
                                out.network_error = Some(classify_reqwest_error(&e));
                                break;
                            }
                        }
                    }
                    _ = &mut deadline => {
                        out.network_error = Some("请求总超时".to_string());
                        break;
                    }
                }
            }
            out.parsed = parser.finish();
        }

        out.total_ms = start.elapsed().as_millis() as u64;
        out
    }
}

/// 响应原文头部脱敏：隐藏常见密钥形态，供诊断展示。
fn redact_head(s: &str) -> String {
    crate::redact::redact_text(s)
}

/// reqwest 错误分类为用户可读信息（不回显 URL 查询参数与密钥）。
fn classify_reqwest_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        return "请求超时".to_string();
    }
    if e.is_connect() {
        return format!("连接失败（代理不可用、DNS 解析失败或服务不可达）: {e}");
    }
    format!("网络错误: {e}")
}
