//! OpenAI Chat Completions 适配器（文档 7.5 / 8.2）。

use serde_json::json;
use serde_json::Value;

use crate::domain::{TestMode, TestTarget};

use super::headers::build_request_headers;
use super::sse::{LineSplitter, SseAggregator, SseEvent};
use super::url::build_endpoint_url;
use super::{ParsedResponse, PreparedRequest, StreamParser};

pub struct OpenAiChatAdapter;

impl OpenAiChatAdapter {
    fn max_tokens_field(compat: &Value) -> &'static str {
        match compat.get("maxTokensField").and_then(|v| v.as_str()) {
            Some("max_completion_tokens") => "max_completion_tokens",
            _ => "max_tokens",
        }
    }
}

impl super::ProtocolAdapter for OpenAiChatAdapter {
    fn protocol(&self) -> crate::domain::ApiProtocol {
        crate::domain::ApiProtocol::OpenAiChat
    }

    fn build_request(
        &self,
        target: &TestTarget,
        prompt: &str,
        mode: TestMode,
        max_tokens: u32,
    ) -> Result<PreparedRequest, String> {
        let url = build_endpoint_url(target, mode)?;
        let built = build_request_headers(target, crate::APP_USER_AGENT, "");
        let mut body = json!({
            "model": target.model_id,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": mode == TestMode::Streaming,
        });
        body[Self::max_tokens_field(&target.compat)] = json!(max_tokens);
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
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            r.error_object = Some(error_summary(err));
        }
        if let Some(content) = v
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
        {
            r.text.push_str(content);
        }
        r.stop_reason = v
            .pointer("/choices/0/finish_reason")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        r
    }

    fn new_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(OpenAiChatStreamParser::new())
    }
}

fn error_summary(err: &Value) -> String {
    let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
    let code = err
        .get("code")
        .map(|c| match c {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default();
    if code.is_empty() {
        message.to_string()
    } else {
        format!("{message} (code: {code})")
    }
}

/// OpenAI Chat 流式：choices[*].delta.content 累积；data: [DONE] 为完成信号。
pub struct OpenAiChatStreamParser {
    splitter: LineSplitter,
    aggregator: SseAggregator,
    out: ParsedResponse,
    done_received: bool,
}

impl OpenAiChatStreamParser {
    fn new() -> Self {
        Self {
            splitter: LineSplitter::new(),
            aggregator: SseAggregator::new(),
            out: ParsedResponse::default(),
            done_received: false,
        }
    }

    fn handle_event(&mut self, ev: SseEvent) {
        self.out.event_count += 1;
        let trimmed = ev.data.trim();
        if trimmed == "[DONE]" {
            self.done_received = true;
            self.out.finish_signal = true;
            return;
        }
        let data: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                self.out.unknown_event_count += 1;
                return;
            }
        };
        if let Some(err) = data.get("error").filter(|e| !e.is_null()) {
            self.out.error_object = Some(error_summary(err));
            return;
        }
        if let Some(choices) = data.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                if let Some(t) = choice.pointer("/delta/content").and_then(|c| c.as_str()) {
                    self.out.text.push_str(t);
                }
            }
        }
        // 最后一个 chunk 的 finish_reason
        if let Some(fr) = data
            .pointer("/choices/0/finish_reason")
            .and_then(|s| s.as_str())
        {
            self.out.stop_reason = Some(fr.to_string());
        }
    }
}

impl StreamParser for OpenAiChatStreamParser {
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
        if !self.done_received && self.out.parse_error.is_none() {
            self.out.parse_error = Some("流式响应未收到 [DONE] 完成信号".to_string());
        }
        std::mem::take(&mut self.out)
    }

    fn text_len(&self) -> usize {
        self.out.text.len()
    }
}
