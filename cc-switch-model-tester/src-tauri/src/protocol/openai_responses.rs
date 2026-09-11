//! OpenAI Responses 适配器（文档 7.5 / 8.3）。

use serde_json::json;
use serde_json::Value;

use crate::domain::{TestMode, TestTarget};

use super::headers::build_request_headers;
use super::sse::{LineSplitter, SseAggregator, SseEvent};
use super::url::build_endpoint_url;
use super::{ParsedResponse, PreparedRequest, StreamParser};

pub struct OpenAiResponsesAdapter;

impl super::ProtocolAdapter for OpenAiResponsesAdapter {
    fn protocol(&self) -> crate::domain::ApiProtocol {
        crate::domain::ApiProtocol::OpenAiResponses
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
        // 文档 7.5：compat 明确禁止 store（supportsStore == false）时删除该字段；
        // 其余情况默认发送 store: false，避免给兼容网关增加无关状态
        let include_store = target
            .compat
            .get("supportsStore")
            .and_then(|v| v.as_bool())
            != Some(false);
        let mut body = json!({
            "model": target.model_id,
            "input": [{
                "role": "user",
                "content": [{ "type": "input_text", "text": prompt }]
            }],
            "max_output_tokens": max_tokens,
            "stream": mode == TestMode::Streaming,
        });
        if include_store {
            body["store"] = json!(false);
        }
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
        // 优先顶层 output_text（部分网关提供）；否则遍历 output[*].content[*].text
        if let Some(t) = v.get("output_text").and_then(|t| t.as_str()) {
            r.text.push_str(t);
        } else if let Some(outputs) = v.get("output").and_then(|o| o.as_array()) {
            for out in outputs {
                if let Some(contents) = out.get("content").and_then(|c| c.as_array()) {
                    for c in contents {
                        if c.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                            if let Some(t) = c.get("text").and_then(|t| t.as_str()) {
                                r.text.push_str(t);
                            }
                        }
                    }
                }
            }
        }
        r.stop_reason = v.get("status").and_then(|s| s.as_str()).map(|s| s.to_string());
        r
    }

    fn new_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(ResponsesStreamParser::new())
    }
}

fn error_summary(err: &Value) -> String {
    let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
    let code = err.get("code").and_then(|c| c.as_str()).unwrap_or("");
    if code.is_empty() {
        message.to_string()
    } else {
        format!("{message} (code: {code})")
    }
}

/// Responses 流式：response.output_text.delta 累积；response.completed / response.failed 为终态。
pub struct ResponsesStreamParser {
    splitter: LineSplitter,
    aggregator: SseAggregator,
    out: ParsedResponse,
}

impl ResponsesStreamParser {
    fn new() -> Self {
        Self {
            splitter: LineSplitter::new(),
            aggregator: SseAggregator::new(),
            out: ParsedResponse::default(),
        }
    }

    fn handle_event(&mut self, ev: SseEvent) {
        self.out.event_count += 1;
        // 事件类型优先取 event 字段，否则从 data 的 type 字段读取
        let event_type = ev
            .event
            .clone()
            .or_else(|| ev.data_parsed().ok().and_then(|v| v.get("type").and_then(|t| t.as_str()).map(|s| s.to_string())));

        let data: Value = match serde_json::from_str(ev.data.trim()) {
            Ok(v) => v,
            Err(_) => {
                self.out.unknown_event_count += 1;
                return;
            }
        };

        match event_type.as_deref() {
            Some("response.output_text.delta") => {
                if let Some(t) = data.get("delta").and_then(|d| d.as_str()) {
                    self.out.text.push_str(t);
                }
            }
            Some("response.completed") => {
                self.out.finish_signal = true;
                if let Some(sr) = data
                    .pointer("/response/status")
                    .and_then(|s| s.as_str())
                {
                    self.out.stop_reason = Some(sr.to_string());
                }
            }
            Some("response.failed") | Some("response.error") => {
                self.out.finish_signal = true;
                let err = data
                    .pointer("/response/error")
                    .filter(|e| !e.is_null())
                    .unwrap_or(&data);
                self.out.error_object = Some(error_summary(err));
            }
            Some(_) => {
                self.out.unknown_event_count += 1;
            }
            None => {
                self.out.unknown_event_count += 1;
            }
        }
    }
}

impl SseEvent {
    fn data_parsed(&self) -> Result<Value, serde_json::Error> {
        serde_json::from_str(self.data.trim())
    }
}

impl StreamParser for ResponsesStreamParser {
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
            self.out.parse_error =
                Some("流式响应未收到 response.completed / response.failed 终态信号".to_string());
        }
        std::mem::take(&mut self.out)
    }

    fn text_len(&self) -> usize {
        self.out.text.len()
    }
}
