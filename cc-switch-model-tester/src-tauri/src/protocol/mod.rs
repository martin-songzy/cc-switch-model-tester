//! 协议适配层（DevelopmentPlan.md 7、8）。
//!
//! 每个协议适配器负责：
//! - 请求构造：URL（7.2）+ Header（7.3）+ 最小 JSON 请求体（7.5）
//! - 响应解析：非流式 JSON / 流式增量解析（第 8 节），与网络层解耦（便于测试）

pub mod anthropic;
pub mod gemini;
pub mod headers;
pub mod openai_chat;
pub mod openai_responses;
pub mod sse;
pub mod url;

use serde::Serialize;

use crate::domain::{ApiProtocol, TestTarget};

pub use crate::domain::TestMode;

/// 默认输出上限与超时（文档 7.1），M4 接入应用设置。
pub const DEFAULT_MAX_TOKENS: u32 = 256;
pub const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
/// 单次响应最大字节数（8 MiB）
pub const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
/// 累计可提取文本最大字符数（1 Mi）
pub const MAX_TEXT_CHARS: usize = 1024 * 1024;

/// 构造完成的请求（结构化 JSON，禁止字符串拼接）。
#[derive(Debug, Clone, Serialize)]
pub struct PreparedRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: serde_json::Value,
}

/// 协议解析结果（网络无关；HTTP 状态分类由执行层/M4 判定引擎负责）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedResponse {
    /// 提取到的模型文本（可跨事件累积）
    pub text: String,
    /// 协议级 error 对象摘要（200 + error 判失败的依据，文档 11.4）
    pub error_object: Option<String>,
    pub stop_reason: Option<String>,
    /// 非流式：JSON 解析成功即 true；流式：收到协议完成信号
    pub finish_signal: bool,
    /// 流式事件/JSON 对象计数
    pub event_count: u32,
    /// 未知事件计数（不因未知事件失败，但保留计数）
    pub unknown_event_count: u32,
    /// JSON/SSE 解析失败说明
    pub parse_error: Option<String>,
    /// 是否因超限截断（响应字节 > 8MiB 或文本 > 1Mi）
    pub truncated: bool,
    /// 是否命中保护上限而中止
    pub aborted: bool,
    /// 已接收响应字节数
    pub response_bytes: u64,
    /// 非 JSON / 解析失败时保留的响应原文开头（脱敏，≤300 字符），用于诊断
    pub raw_body_head: Option<String>,
}

impl ParsedResponse {
    pub fn has_text(&self) -> bool {
        !self.text.trim().is_empty()
    }
}

/// 流式解析器：由执行层喂数据，与网络解耦。
pub trait StreamParser: Send {
    /// SSE 协议：喂入一行（Anthropic / OpenAI Chat / OpenAI Responses）
    fn feed_line(&mut self, line: &str);
    /// 字节流（Gemini 多 JSON 对象流 / SSE 字节流统一入口）
    fn feed_chunk(&mut self, chunk: &[u8]) {
        let _ = chunk;
    }
    /// 流结束（EOF）时冲出最终结果
    fn finish(&mut self) -> ParsedResponse;

    /// 当前已累积文本长度（执行层用于 1 Mi 上限检查）
    fn text_len(&self) -> usize {
        0
    }
}

/// 协议适配器。
pub trait ProtocolAdapter: Send + Sync {
    fn protocol(&self) -> ApiProtocol;

    fn build_request(
        &self,
        target: &TestTarget,
        prompt: &str,
        mode: TestMode,
        max_tokens: u32,
    ) -> Result<PreparedRequest, String>;

    /// 非流式响应解析（body 已按 8MiB 上限读取）
    fn parse_non_stream(&self, body: &[u8]) -> ParsedResponse;

    fn new_stream_parser(&self) -> Box<dyn StreamParser>;
}

pub fn adapter_for(protocol: ApiProtocol) -> Box<dyn ProtocolAdapter> {
    match protocol {
        ApiProtocol::AnthropicMessages => Box::new(anthropic::AnthropicAdapter),
        ApiProtocol::OpenAiChat => Box::new(openai_chat::OpenAiChatAdapter),
        ApiProtocol::OpenAiResponses => Box::new(openai_responses::OpenAiResponsesAdapter),
        ApiProtocol::GeminiNative => Box::new(gemini::GeminiAdapter),
        ApiProtocol::BedrockConverseStream => {
            unreachable!("Bedrock 不构造请求（执行层应先行拦截）")
        }
    }
}

/// 统一入口：构造请求（URL + Header + Body）。
pub fn build_request(
    target: &TestTarget,
    prompt: &str,
    mode: TestMode,
    max_tokens: u32,
) -> Result<PreparedRequest, String> {
    let adapter = adapter_for(target.protocol);
    adapter.build_request(target, prompt, mode, max_tokens)
}
