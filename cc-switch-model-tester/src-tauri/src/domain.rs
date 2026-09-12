//! 统一领域模型（DevelopmentPlan.md 5.7 节的 M2 子集）。
//!
//! 两类结构必须严格区分：
//! - `ProviderSnapshot` / `CredentialValue`：含明文凭据，只存在于 Rust 内存，
//!   不得实现 `Serialize` 到前端；
//! - `*View` 结构：脱敏后发送给前端。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::redact;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppType {
    Claude,
    Codex,
    Pi,
}

impl AppType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppType::Claude => "claude",
            AppType::Codex => "codex",
            AppType::Pi => "pi",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(AppType::Claude),
            "codex" => Some(AppType::Codex),
            "pi" => Some(AppType::Pi),
            _ => None,
        }
    }
}

/// 上游协议。Pi 的协议值在解析层归一化到对应通用协议（文档 6.3 / 10.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProtocol {
    AnthropicMessages,
    OpenAiChat,
    OpenAiResponses,
    GeminiNative,
    BedrockConverseStream,
}

impl ApiProtocol {
    pub fn display_name(&self) -> &'static str {
        match self {
            ApiProtocol::AnthropicMessages => "Anthropic Messages",
            ApiProtocol::OpenAiChat => "OpenAI Chat",
            ApiProtocol::OpenAiResponses => "OpenAI Responses",
            ApiProtocol::GeminiNative => "Gemini Native",
            ApiProtocol::BedrockConverseStream => "Bedrock ConverseStream",
        }
    }

    /// 存入本地历史库的统一小写串。
    pub fn db_str(&self) -> &'static str {
        match self {
            ApiProtocol::AnthropicMessages => "anthropic_messages",
            ApiProtocol::OpenAiChat => "openai_chat",
            ApiProtocol::OpenAiResponses => "openai_responses",
            ApiProtocol::GeminiNative => "gemini_native",
            ApiProtocol::BedrockConverseStream => "bedrock_converse_stream",
        }
    }
}

/// 凭据形态。决定 M3 请求构造时使用的认证 Header。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    /// ANTHROPIC_AUTH_TOKEN → `Authorization: Bearer`
    AuthToken,
    /// ANTHROPIC_API_KEY → `x-api-key`
    ApiKeyHeader,
    /// OpenAI / Pi 兼容 Key → `Authorization: Bearer`
    BearerKey,
    /// GEMINI_API_KEY → `x-goog-api-key`
    GeminiKey,
    /// 供应商配置 headers 中已显式携带认证（保留用户显式值）
    ExplicitHeader,
    /// 托管认证（OAuth 等），第一版不支持
    Managed,
    /// 缺失
    Missing,
}

impl CredentialKind {
    /// UI 展示标签（只显示状态，绝不显示密钥）。
    pub fn ui_label(&self) -> &'static str {
        match self {
            CredentialKind::Managed => "托管认证",
            CredentialKind::Missing => "未配置",
            _ => "已配置",
        }
    }
}

/// Rust 内存中的完整凭据值（不可序列化外发）。
#[derive(Debug, Clone)]
pub struct CredentialValue {
    pub kind: CredentialKind,
    pub secret: String,
}

/// 供应商可测状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    /// 可测试
    Ready,
    /// 配置错误（不能进入可执行队列）
    ConfigError,
    /// 托管认证，第一版跳过
    ManagedAuthSkipped,
    /// 协议暂不支持（Bedrock）
    UnsupportedProtocol,
}

impl ProviderStatus {
    pub fn ui_label(&self) -> &'static str {
        match self {
            ProviderStatus::Ready => "可测试",
            ProviderStatus::ConfigError => "配置错误",
            ProviderStatus::ManagedAuthSkipped => "已跳过：托管认证",
            ProviderStatus::UnsupportedProtocol => "暂不支持",
        }
    }
}

/// 模型快照（无敏感信息）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSnapshot {
    /// 实际请求发送的模型 ID（已剥离 cc-switch 展示标记）
    pub model_id: String,
    pub display_name: Option<String>,
    /// 模型级 baseUrl 覆盖（Pi）
    pub base_url_override: Option<String>,
    /// compat 合并结果（供应商级 + 模型级，模型级优先）
    pub compat: serde_json::Value,
    /// Pi 的思考级别映射（原样保留，仅作兼容性元数据）
    pub thinking_level_map: Option<serde_json::Value>,
    /// 从模型 ID 剥离出的 cc-switch 展示标记（如 "1M"），仅元数据
    pub id_markers: Vec<String>,
}

/// 供应商完整快照（含明文凭据，仅 Rust 内存）。
#[derive(Debug, Clone)]
pub struct ProviderSnapshot {
    pub app: AppType,
    pub provider_id: String,
    pub provider_name: String,
    /// 主端点（来自 settings_config 的 Base URL）
    pub endpoint: String,
    /// 同库 provider_endpoints 候选端点（默认不测试，仅展示）
    pub candidate_endpoints: Vec<String>,
    pub protocol: ApiProtocol,
    /// 原始协议串（如 "anthropic-messages"），显示用
    pub raw_protocol: Option<String>,
    pub models: Vec<ModelSnapshot>,
    pub credential: Option<CredentialValue>,
    /// 供应商字面量自定义 Header（Pi headers / Codex http_headers / Claude overrides.headers）
    pub headers: Vec<(String, String)>,
    pub custom_user_agent: Option<String>,
    /// pi 供应商 settings_config 里的 clientEmulation 配置（None = 未配置）
    pub client_emulation: Option<ClientEmulationSpec>,
    /// meta.localProxyRequestOverrides.body（cc-switch 本地代理请求体改写规则）
    pub local_proxy_body_patch: Option<serde_json::Value>,
    /// meta.isFullUrl = true 时不自动追加 API 路径
    pub full_url: bool,
    /// compat 合并结果
    pub compat: serde_json::Value,
    /// 未知字段 passthrough（绝不写回 cc-switch）
    pub passthrough: serde_json::Value,
    /// settings_config 原文的 SHA-256（配置变化检测）
    pub raw_config_hash: String,
    pub status: ProviderStatus,
    pub warnings: Vec<String>,
    /// status = ConfigError 时的具体原因（用户可读）
    pub config_error: Option<String>,
}

impl ProviderSnapshot {
    /// 前端视图（脱敏）：不含 settings_config 原文、不含明文凭据、URL query 脱敏。
    pub fn to_catalog_view(&self) -> ProviderCatalogView {
        let credential_label = match (&self.credential, self.status) {
            (_, ProviderStatus::ManagedAuthSkipped) => CredentialKind::Managed.ui_label(),
            (Some(c), _) => c.kind.ui_label(),
            (None, _) => CredentialKind::Missing.ui_label(),
        };
        ProviderCatalogView {
            id: self.provider_id.clone(),
            app_type: self.app.as_str().to_string(),
            name: self.provider_name.clone(),
            status: self.status,
            status_label: self.status.ui_label().to_string(),
            protocol: self.protocol,
            raw_protocol: self.raw_protocol.clone(),
            endpoint_display: redact::redact_url(&self.endpoint),
            candidate_endpoint_count: self.candidate_endpoints.len(),
            models: self
                .models
                .iter()
                .map(|m| ModelView {
                    model_id: m.model_id.clone(),
                    display_name: m.display_name.clone(),
                })
                .collect(),
            credential_label: credential_label.to_string(),
            client_emulation: self.client_emulation.as_ref().map(|s| ClientEmulationSpecView {
                enabled: s.enabled,
                profile: s.profile.clone(),
            }),
            warnings: self.warnings.clone(),
            error: self.config_error.clone(),
            raw_config_hash: self.raw_config_hash.clone(),
        }
    }
}

/// 供应商目录视图（发给前端，脱敏）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogView {
    pub id: String,
    pub app_type: String,
    pub name: String,
    pub status: ProviderStatus,
    pub status_label: String,
    pub protocol: ApiProtocol,
    pub raw_protocol: Option<String>,
    pub endpoint_display: String,
    pub candidate_endpoint_count: usize,
    pub models: Vec<ModelView>,
    pub credential_label: String,
    /// 客户端仿真配置（pi 供应商；None = 未配置）
    pub client_emulation: Option<ClientEmulationSpecView>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub raw_config_hash: String,
}

/// 客户端仿真配置摘要（目录页展示 + 默认开关值）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientEmulationSpecView {
    /// 配置默认状态
    pub enabled: bool,
    /// 画像名
    pub profile: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    pub model_id: String,
    pub display_name: Option<String>,
}

/// 测试模式：快速非流式 / 兼容性流式（文档 7.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestMode {
    NonStreaming,
    Streaming,
}

/// 单个测试目标（供应商快照 × 选定模型），M3/M4 核心输入。
#[derive(Debug, Clone)]
pub struct TestTarget {
    pub app: AppType,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_display_name: Option<String>,
    pub endpoint_url: String,
    pub protocol: ApiProtocol,
    pub credential: Option<CredentialValue>,
    pub headers: Vec<(String, String)>,
    pub custom_user_agent: Option<String>,
    /// 客户端仿真（每模型开关，默认值来自供应商配置）
    pub emulation: bool,
    /// 用户显式选择的画像名；None = 按供应商配置/协议默认推导
    pub emulation_profile: Option<String>,
    pub client_emulation: Option<ClientEmulationSpec>,
    /// cc-switch 本地代理 body 改写规则（应用与否由全局开关控制）
    pub local_proxy_body_patch: Option<serde_json::Value>,
    pub full_url: bool,
    pub compat: serde_json::Value,
}

/// 客户端仿真配置（来自 pi 供应商 settings_config 的 clientEmulation 字段）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientEmulationSpec {
    /// 配置默认状态（clientEmulation.enabled）
    pub enabled: bool,
    /// 画像名：claude-code / codex / gemini-cli
    pub profile: String,
    /// 覆盖 UA
    pub user_agent: Option<String>,
    /// 追加/覆盖请求头；值 None 表示删除该头
    pub headers: Vec<(String, Option<String>)>,
}

impl ProviderSnapshot {
    /// 由快照与选定模型生成测试目标（模型级 baseUrl 覆盖在此应用）。
    pub fn to_test_target(&self, model: &ModelSnapshot) -> TestTarget {
        TestTarget {
            app: self.app,
            provider_id: self.provider_id.clone(),
            provider_name: self.provider_name.clone(),
            model_id: model.model_id.clone(),
            model_display_name: model.display_name.clone(),
            endpoint_url: model
                .base_url_override
                .clone()
                .unwrap_or_else(|| self.endpoint.clone()),
            protocol: self.protocol,
            credential: self.credential.clone(),
            headers: self.headers.clone(),
            custom_user_agent: self.custom_user_agent.clone(),
            emulation: false, // 由调度层按每模型选择赋值
            emulation_profile: None,
            client_emulation: self.client_emulation.clone(),
            local_proxy_body_patch: self.local_proxy_body_patch.clone(),
            full_url: self.full_url,
            compat: model.compat.clone(),
        }
    }
}

/// 计算配置原文哈希（SHA-256 hex）。
pub fn config_hash(raw: &str) -> String {
    let mut h = Sha256::new();
    h.update(raw.as_bytes());
    let out = h.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// 剥离 cc-switch 模型 ID 展示标记（如 `claude-x[1M]` → `claude-x`，标记 `1M` 记为元数据）。
pub fn strip_model_id_markers(raw_id: &str) -> (String, Vec<String>) {
    let mut id = raw_id.trim().to_string();
    let mut markers = Vec::new();
    // 形式：[XXX] 结尾，可连续多个
    loop {
        let Some(start) = id.rfind('[') else { break };
        if !id.ends_with(']') || start + 1 >= id.len() - 1 {
            break;
        }
        let marker = id[start + 1..id.len() - 1].trim().to_string();
        if marker.is_empty() {
            break;
        }
        markers.push(marker);
        id.truncate(start);
    }
    if markers.is_empty() {
        (id, markers)
    } else {
        markers.reverse();
        (id, markers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_hex() {
        let h1 = config_hash("abc");
        let h2 = config_hash("abc");
        let h3 = config_hash("abd");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn strips_markers() {
        let (id, m) = strip_model_id_markers("claude-opus-5[1M]");
        assert_eq!(id, "claude-opus-5");
        assert_eq!(m, vec!["1M"]);

        let (id, m) = strip_model_id_markers("gpt-x[1m][beta]");
        assert_eq!(id, "gpt-x");
        assert_eq!(m, vec!["1m", "beta"]);

        let (id, m) = strip_model_id_markers("plain-model");
        assert_eq!(id, "plain-model");
        assert!(m.is_empty());

        // 非标记形式的方括号不剥离
        let (id, m) = strip_model_id_markers("weird[name-only");
        assert_eq!(id, "weird[name-only");
        assert!(m.is_empty());
    }
}
