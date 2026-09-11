//! Gemini Native (generateContent / streamGenerateContent) 适配器（文档 7.5 / 8.4）。
//!
//! 流式响应可能是多个连续 JSON 对象（默认）或 SSE（alt=sse），两种都要支持。

use serde_json::json;
use serde_json::Value;

use crate::domain::{TestMode, TestTarget};

use super::headers::build_request_headers;
use super::sse::{LineSplitter, SseAggregator};
use super::url::build_endpoint_url;
use super::{ParsedResponse, PreparedRequest, StreamParser};

pub struct GeminiAdapter;

impl GeminiAdapter {
    /// 从单个 JSON 对象提取文本与状态（非流式与流式共用）。
    pub(crate) fn extract(v: &Value, out: &mut ParsedResponse) {
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
            let status = err.get("status").and_then(|s| s.as_str()).unwrap_or("");
            out.error_object = Some(if status.is_empty() {
                message.to_string()
            } else {
                format!("{message} (status: {status})")
            });
        }
        if let Some(candidates) = v.get("candidates").and_then(|c| c.as_array()) {
            for cand in candidates {
                if let Some(parts) = cand.pointer("/content/parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                            out.text.push_str(t);
                        }
                    }
                }
                if let Some(fr) = cand.get("finishReason").and_then(|f| f.as_str()) {
                    out.stop_reason = Some(fr.to_string());
                }
            }
        }
        // promptFeedback.blockReason 表示被安全拦截，按协议错误记录
        if let Some(block) = v.pointer("/promptFeedback/blockReason").and_then(|b| b.as_str()) {
            out.error_object = Some(format!("promptFeedback.blockReason: {block}"));
        }
    }
}

impl super::ProtocolAdapter for GeminiAdapter {
    fn protocol(&self) -> crate::domain::ApiProtocol {
        crate::domain::ApiProtocol::GeminiNative
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
        let body = json!({
            "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
            "generationConfig": { "maxOutputTokens": max_tokens },
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
        Self::extract(&v, &mut r);
        r
    }

    fn new_stream_parser(&self) -> Box<dyn StreamParser> {
        Box::new(GeminiStreamParser::new())
    }
}

/// Gemini 流式解析：
/// - SSE 形态（data: {json} 行）
/// - 连续 JSON 对象字节流（无换行分隔也能处理）
/// 通过流开头首个非空白字节判断形态。
pub struct GeminiStreamParser {
    mode_detected: Option<bool>, // Some(true)=SSE
    splitter: LineSplitter,
    aggregator: SseAggregator,
    json_buf: Vec<u8>,
    out: ParsedResponse,
}

impl GeminiStreamParser {
    fn new() -> Self {
        Self {
            mode_detected: None,
            splitter: LineSplitter::new(),
            aggregator: SseAggregator::new(),
            json_buf: Vec::new(),
            out: ParsedResponse::default(),
        }
    }

    fn handle_json(&mut self, bytes: &[u8]) {
        self.out.event_count += 1;
        match serde_json::from_slice::<Value>(bytes) {
            Ok(v) => GeminiAdapter::extract(&v, &mut self.out),
            Err(e) => {
                self.out.parse_error = Some(format!("JSON 对象解析失败: {e}"));
            }
        }
    }

    /// 扫描 json_buf，弹出所有平衡完整的顶层 JSON 对象。
    fn drain_json_objects(&mut self) {
        loop {
            // 跳过前导空白
            while let Some(&b) = self.json_buf.first() {
                if b.is_ascii_whitespace() {
                    self.json_buf.remove(0);
                } else {
                    break;
                }
            }
            if self.json_buf.first() != Some(&b'{') {
                if !self.json_buf.is_empty() {
                    self.out.parse_error = Some("流式 JSON 缓冲不以对象开头".to_string());
                    self.json_buf.clear();
                }
                return;
            }
            // 字符串感知的深度扫描
            let mut depth = 0i32;
            let mut in_str = false;
            let mut esc = false;
            let mut end: Option<usize> = None;
            for (i, &b) in self.json_buf.iter().enumerate() {
                if in_str {
                    if esc {
                        esc = false;
                    } else if b == b'\\' {
                        esc = true;
                    } else if b == b'"' {
                        in_str = false;
                    }
                    continue;
                }
                match b {
                    b'"' => in_str = true,
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(i + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            match end {
                Some(n) => {
                    let obj: Vec<u8> = self.json_buf.drain(..n).collect();
                    self.handle_json(&obj);
                }
                None => return, // 不完整，等待更多数据
            }
        }
    }
}

impl StreamParser for GeminiStreamParser {
    fn feed_chunk(&mut self, chunk: &[u8]) {
        self.out.response_bytes += chunk.len() as u64;
        if self.mode_detected.is_none() {
            let first_non_ws = chunk.iter().find(|&&b| !b.is_ascii_whitespace()).copied();
            if let Some(b) = first_non_ws {
                self.mode_detected = Some(b == b'd'); // data: → SSE
            }
        }
        match self.mode_detected {
            Some(true) => {
                for line in self.splitter.feed(chunk) {
                    self.out.event_count += 0; // 事件计数在 handle_json
                    if let Some(ev) = self.aggregator.feed_line(&line) {
                        if ev.data.trim() == "[DONE]" {
                            self.out.finish_signal = true;
                            continue;
                        }
                        self.handle_json(ev.data.as_bytes());
                    }
                }
            }
            Some(false) => {
                self.json_buf.extend_from_slice(chunk);
                self.drain_json_objects();
            }
            None => {
                // 只有空白，先缓冲
                self.json_buf.extend_from_slice(chunk);
            }
        }
    }

    fn feed_line(&mut self, line: &str) {
        // 兼容直接按行喂数据（测试便捷）
        let mut b = line.as_bytes().to_vec();
        b.push(b'\n');
        self.feed_chunk(&b);
    }

    fn finish(&mut self) -> ParsedResponse {
        if self.mode_detected == Some(true) {
            if let Some(line) = self.splitter.flush() {
                if let Some(ev) = self.aggregator.feed_line(&line) {
                    self.handle_json(ev.data.as_bytes());
                }
            }
            if let Some(ev) = self.aggregator.flush() {
                self.handle_json(ev.data.as_bytes());
            }
        } else {
            // 残留 JSON
            if !self.json_buf.iter().all(|b| b.is_ascii_whitespace()) {
                let rest = std::mem::take(&mut self.json_buf);
                self.handle_json(&rest);
            }
        }
        // Gemini 完成信号：HTTP EOF 且无解析错误（文档 8.6）
        if self.out.parse_error.is_none() {
            self.out.finish_signal = true;
        }
        std::mem::take(&mut self.out)
    }

    fn text_len(&self) -> usize {
        self.out.text.len()
    }
}
