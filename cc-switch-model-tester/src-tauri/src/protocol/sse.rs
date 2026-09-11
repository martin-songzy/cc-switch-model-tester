//! 通用 SSE（text/event-stream）解析（DevelopmentPlan.md 8.6）。
//!
//! 网络无关：执行层把字节流切块喂进来，这里负责跨块行缓冲与事件切分。

/// 跨 chunk 的行缓冲：feed 任意字节片段，吐出完整行（不含行尾符）。
pub struct LineSplitter {
    buf: Vec<u8>,
}

impl LineSplitter {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// 喂入一块字节，返回其中完整行（按 \n 分割，剥离 \r）。
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut lines = Vec::new();
        loop {
            match self.buf.iter().position(|&b| b == b'\n') {
                Some(pos) => {
                    let line: Vec<u8> = self.buf.drain(..=pos).collect();
                    let mut line = &line[..line.len() - 1]; // 去掉 \n
                    if line.last() == Some(&b'\r') {
                        line = &line[..line.len() - 1]; // 去掉 \r
                    }
                    lines.push(String::from_utf8_lossy(line).into_owned());
                }
                None => break,
            }
        }
        lines
    }

    /// 流结束时冲出残留（无换行符的最后一行）。
    pub fn flush(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let line = String::from_utf8_lossy(&self.buf).into_owned();
        self.buf.clear();
        Some(line)
    }
}

/// 一个 SSE 事件（event 字段 + data 内容）。
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// 把连续的 SSE 行聚合成事件：`event:` 设定类型，`data:` 追加内容，空行分派。
pub struct SseAggregator {
    current_event: Option<String>,
    data_parts: Vec<String>,
}

impl SseAggregator {
    pub fn new() -> Self {
        Self {
            current_event: None,
            data_parts: Vec::new(),
        }
    }

    /// 处理一行；返回 Some 表示一个完整事件已就绪。
    pub fn feed_line(&mut self, line: &str) -> Option<SseEvent> {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.is_empty() {
            return self.take_event();
        }
        if let Some(rest) = trimmed.strip_prefix("event:") {
            self.current_event = Some(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("data:") {
            self.data_parts.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
        // 其余行（注释、id:、retry: 等）忽略
        None
    }

    fn take_event(&mut self) -> Option<SseEvent> {
        if self.current_event.is_none() && self.data_parts.is_empty() {
            return None;
        }
        let ev = SseEvent {
            event: self.current_event.take(),
            data: std::mem::take(&mut self.data_parts).join("\n"),
        };
        Some(ev)
    }

    /// 流结束时冲出未以空行结尾的最后一个事件。
    pub fn flush(&mut self) -> Option<SseEvent> {
        self.take_event()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_across_chunks() {
        let mut ls = LineSplitter::new();
        let l1 = ls.feed(b"data: {\"a\":");
        assert!(l1.is_empty());
        let l2 = ls.feed(b"1}\n\ndata: [DONE]\n");
        assert_eq!(l2, vec!["data: {\"a\":1}", "", "data: [DONE]"]);
        assert_eq!(ls.flush(), None);
    }

    #[test]
    fn aggregates_events() {
        let mut agg = SseAggregator::new();
        assert!(agg.feed_line("event: content_block_delta").is_none());
        assert!(agg.feed_line("data: {\"d\":1}").is_none());
        let ev = agg.feed_line("").unwrap();
        assert_eq!(ev.event.as_deref(), Some("content_block_delta"));
        assert_eq!(ev.data, "{\"d\":1}");
        // 无 event 字段的事件
        assert!(agg.feed_line("data: {\"x\":2}").is_none());
        let ev2 = agg.flush().unwrap();
        assert_eq!(ev2.event, None);
        assert_eq!(ev2.data, "{\"x\":2}");
    }
}
