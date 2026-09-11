//! Anthropic Messages 适配器（文档 7.5 / 8.1）。

use serde_json::json;
use serde_json::Value;

use crate::domain::{TestMode, TestTarget};

use super::headers::build_request_headers;
use super::url::build_endpoint_url;
use super::{ParsedResponse, PreparedRequest, StreamParser, DEFAULT_ANTHROPIC_VERSION};

pub struct AnthropicAdapter;

impl super::ProtocolAdapter for AnthropicAdapter {
    fn protocol(&self) -> crate::domain::ApiProtocol {
        crate::domain::ApiProtocol::AnthropicMessages
    }

    fn build_request(
        &self,
        target: &TestTarget,
        prompt: &str,
        mode: TestMode,
        max_tokens: u32,
    ) -> Result<PreparedRequest, String> {
        let url = build_endpoint_url(target, mode)?;
        let built = build_request_headers(target, crate::APP_USER_AGENT, DEFAULT_ANTHROPIC_VERSION);
        let body = json!({
            "model": target.model_id,
            "max_tokens": max_tokens,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": mode == TestMode::Streaming,
        });
        Ok(PreparedRequest {
            url,
            headers: built.headers,
            body,
        })
    }

    fn parse_non_stream(&self, body: &[u8]) -> ParsedResponse {
        let mut r = ParsedResponse::default();
        let v: Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(e) => {
                r.parse_error = Some(format!("JSON 解析失败: {e}"));
                return r;
            }
        };
        r.finish_signal = true;
        if let Some(err) = extract_error(&v) {
            r.error_object = Some(err);
        }
        if let Some(arr) = v.get("content").and_then(|c| c.as_array()) {
            for item in arr {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        r.text.push_str(t);
                    }
                }
            }
        }
        r.stop_reason = v
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        r
    }

    fn new_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(AnthropicStreamParser::new())
    }
}

fn extract_error(v: &Value) -> Option<String> {
    // 非流式顶层 error；流式 error 事件 data
    let err = v.get("error")?;
    if err.is_null() {
        return None;
    }
    let type_field = err.get("type").and_then(|t| t.as_str()).unwrap_or("error");
    let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
    Some(format!("{type_field}: {message}"))
}

/// Anthropic 流式解析：content_block_delta / message_delta / message_stop / error。
pub struct AnthropicStreamParser {
    splitter: super::sse::LineSplitter,
    aggregator: super::sse::SseAggregator,
    out: ParsedResponse,
}

impl AnthropicStreamParser {
    fn new() -> Self {
        Self {
            splitter: super::sse::LineSplitter::new(),
            aggregator: super::sse::SseAggregator::new(),
            out: ParsedResponse::default(),
        }
    }

    fn handle_event(&mut self, ev: super::sse::SseEvent) {
        self.out.event_count += 1;
        let event_type = ev.event.as_deref();
        let data: Value = match serde_json::from_str(&ev.data) {
            Ok(v) => v,
            Err(_) => {
                // [DONE] 等非 JSON data 不应出现在 Anthropic；计未知
                self.out.unknown_event_count += 1;
                return;
            }
        };
        match event_type {
            Some("content_block_delta") => {
                if let Some(t) = data
                    .pointer("/delta/text")
                    .and_then(|t| t.as_str())
                {
                    self.out.text.push_str(t);
                }
            }
            Some("message_delta") => {
                if let Some(sr) = data.pointer("/delta/stop_reason").and_then(|s| s.as_str()) {
                    self.out.stop_reason = Some(sr.to_string());
                }
            }
            Some("message_stop") => {
                self.out.finish_signal = true;
            }
            Some("error") => {
                if let Some(err) = extract_error(&data) {
                    self.out.error_object = Some(err);
                }
            }
            Some("ping") | Some("message_start") | Some("content_block_start")
            | Some("content_block_stop") => { /* 已计数，无需处理 */ }
            _ => {
                self.out.unknown_event_count += 1;
            }
        }
    }
}

impl StreamParser for AnthropicStreamParser {
    fn feed_line(&mut self, line: &str) {
        if let Some(ev) = self.aggregator.feed_line(line) {
            self.handle_event(ev);
        }
    }

    fn feed_chunk(&mut self, chunk: &[u8]) {
        self.out.response_bytes += chunk.len() as u64;
        for line in self.splitter.feed(chunk) {
            self.feed_line(&line);
        }
    }

    fn finish(&mut self) -> ParsedResponse {
        if let Some(line) = self.splitter.flush() {
            self.feed_line(&line);
        }
        if let Some(ev) = self.aggregator.flush() {
            self.handle_event(ev);
        }
        if !self.out.finish_signal && self.out.parse_error.is_none() {
            self.out.parse_error = Some("流式响应未收到 message_stop 完成信号".to_string());
        }
        std::mem::take(&mut self.out)
    }

    fn text_len(&self) -> usize {
        self.out.text.len()
    }
}
