//! 解析器集成测试：基于脱敏 fixture（源自真实 cc-switch 数据）+ 合成用例，
//! 覆盖 DevelopmentPlan.md 第 6 节的解析规则与 19.1 单测要求中的解析部分。

use cc_switch_model_tester_lib::domain::{
    AppType, CredentialKind, ProviderSnapshot, ProviderStatus,
};
use cc_switch_model_tester_lib::parser::parse_provider;
use serde_json::json;

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

struct Fixture {
    app: AppType,
    provider_id: String,
    name: String,
    settings_config: String,
    meta: String,
}

fn load_fixture(file: &str) -> Fixture {
    let text = std::fs::read_to_string(format!("{FIXTURE_DIR}/{file}"))
        .unwrap_or_else(|e| panic!("读取 fixture {file} 失败: {e}"));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    Fixture {
        app: AppType::from_db_str(v["app_type"].as_str().unwrap()).unwrap(),
        provider_id: v["provider_id"].as_str().unwrap().to_string(),
        name: v["name"].as_str().unwrap().to_string(),
        settings_config: v["settings_config"].as_str().unwrap().to_string(),
        meta: v["meta"].as_str().unwrap().to_string(),
    }
}

fn parse(f: &Fixture) -> ProviderSnapshot {
    parse_provider(
        f.app,
        &f.provider_id,
        &f.name,
        &f.settings_config,
        &f.meta,
        Vec::new(),
    )
}

// ==================== Claude fixture ====================

#[test]
fn claude_anthropic_relay_b_ready() {
    let snap = parse(&load_fixture("claude-anthropic-relay-b.json"));
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert_eq!(snap.app, AppType::Claude);
    assert_eq!(snap.protocol, cc_switch_model_tester_lib::domain::ApiProtocol::AnthropicMessages);
    assert!(!snap.models.is_empty(), "应从 env 模型字段收集到模型");
    // *_MODEL_NAME 只做显示名，不能成为模型 ID
    assert!(
        snap.models.iter().all(|m| !m.model_id.ends_with("_NAME")),
        "模型 ID 不应来自 *_MODEL_NAME 字段"
    );
    // 模型去重
    let ids: std::collections::HashSet<_> = snap.models.iter().map(|m| m.model_id.clone()).collect();
    assert_eq!(ids.len(), snap.models.len());
    assert!(snap.credential.is_some(), "relay-b fixture 含认证字段");
    assert!(snap.custom_user_agent.is_none());
}

#[test]
fn claude_relay_a_custom_user_agent() {
    let snap = parse(&load_fixture("claude-anthropic-relay-a.json"));
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert!(
        snap.custom_user_agent
            .as_deref()
            .is_some_and(|ua| ua.starts_with("claude-cli/")),
        "meta.customUserAgent 应被读取"
    );
    assert!(!snap.endpoint.is_empty());
    assert!(snap.endpoint.starts_with("https://"));
}

#[test]
fn claude_openai_chat_protocol_mapping() {
    let snap = parse(&load_fixture("claude-openai-chat-relay-c.json"));
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert_eq!(snap.protocol, cc_switch_model_tester_lib::domain::ApiProtocol::OpenAiChat);
    assert_eq!(snap.raw_protocol.as_deref(), Some("openai_chat"));
}

#[test]
fn claude_openai_responses_protocol_mapping() {
    let snap = parse(&load_fixture("claude-openai-responses-relay-d.json"));
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert_eq!(snap.protocol, cc_switch_model_tester_lib::domain::ApiProtocol::OpenAiResponses);
}

// ==================== Codex fixture ====================

#[test]
fn codex_relay_a_toml_and_catalog() {
    let snap = parse(&load_fixture("codex-relay-a.json"));
    assert_eq!(snap.status, ProviderStatus::Ready, "error={:?}", snap.config_error);
    assert_eq!(snap.protocol, cc_switch_model_tester_lib::domain::ApiProtocol::OpenAiResponses);
    assert_eq!(snap.endpoint, "https://relay-a.example.com/v1");
    assert_eq!(snap.models.len(), 1);
    assert_eq!(snap.models[0].model_id, "gpt-5.5");
    assert!(snap.credential.is_some());
    assert_eq!(snap.credential.as_ref().unwrap().kind, CredentialKind::BearerKey);
}

#[test]
fn codex_official_no_model_is_config_error() {
    let snap = parse(&load_fixture("codex-official.json"));
    // 官方供应商 config 无 base_url/model/modelCatalog：不可测（先报缺 Base URL）
    assert_eq!(snap.status, ProviderStatus::ConfigError);
    let err = snap.config_error.unwrap();
    assert!(err.contains("缺少 Base URL") || err.contains("模型列表为空"), "实际: {err}");
}

#[test]
fn codex_base_url_but_no_model_is_config_error() {
    // 有 base_url 但无任何模型来源 → “模型列表为空”
    let sc = json!({
        "auth": { "OPENAI_API_KEY": "sk-test" },
        "config": "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"https://x\"\n"
    });
    let snap = parse_provider(AppType::Codex, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::ConfigError);
    assert!(snap.config_error.unwrap().contains("模型列表为空"));
}

// ==================== Pi fixture ====================

#[test]
fn pi_anthropic_messages_relay_e() {
    let snap = parse(&load_fixture("pi-anthropic-messages-relay-e.json"));
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert_eq!(snap.protocol, cc_switch_model_tester_lib::domain::ApiProtocol::AnthropicMessages);
    assert_eq!(snap.raw_protocol.as_deref(), Some("anthropic-messages"));
    assert_eq!(snap.models.len(), 1);
    assert_eq!(snap.models[0].model_id, "claude-opus-5");
    // anthropic-messages 协议默认 x-api-key 认证
    assert_eq!(snap.credential.as_ref().unwrap().kind, CredentialKind::ApiKeyHeader);
}

#[test]
fn pi_openai_completions_relay_g() {
    let snap = parse(&load_fixture("pi-openai-completions-relay-g.json"));
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert_eq!(snap.protocol, cc_switch_model_tester_lib::domain::ApiProtocol::OpenAiChat);
    assert_eq!(snap.models.len(), 3);
    assert_eq!(snap.models[0].model_id, "glm-5.3-flash");
    // thinkingLevelMap 原样保留
    assert!(
        snap.models[0].thinking_level_map.is_some(),
        "thinkingLevelMap 应保留"
    );
    // openai 协议默认 Bearer
    assert_eq!(snap.credential.as_ref().unwrap().kind, CredentialKind::BearerKey);
}

#[test]
fn pi_openai_responses_relay_f() {
    let snap = parse(&load_fixture("pi-openai-responses-relay-f.json"));
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert_eq!(snap.protocol, cc_switch_model_tester_lib::domain::ApiProtocol::OpenAiResponses);
    assert!(
        snap.warnings.iter().any(|w| w.contains("clientEmulation")),
        "clientEmulation 应产生直连语义警告"
    );
}

#[test]
fn pi_explicit_auth_header_wins() {
    let snap = parse(&load_fixture("pi-authheader-clientemulation.json"));
    assert_eq!(snap.status, ProviderStatus::Ready);
    // headers.Authorization 显式存在 → ExplicitHeader（保留用户显式值）
    assert_eq!(snap.credential.as_ref().unwrap().kind, CredentialKind::ExplicitHeader);
    assert_eq!(snap.models.len(), 3);
    assert!(snap.headers.iter().any(|(k, _)| k == "Authorization"));
}

// ==================== 合成用例 ====================

#[test]
fn pi_authheader_boolean_switch_uses_bearer() {
    // Pi 官方语义：authHeader=true → Authorization: Bearer <apiKey>
    let sc = json!({
        "name": "test", "baseUrl": "https://x.example", "api": "anthropic-messages",
        "apiKey": "sk-test", "authHeader": true,
        "models": [{ "id": "m1" }]
    });
    let snap = parse_provider(AppType::Pi, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::Ready, "error={:?}", snap.config_error);
    assert_eq!(snap.credential.as_ref().unwrap().kind, CredentialKind::AuthToken);
}

#[test]
fn pi_model_base_url_override_and_compat_merge() {
    let sc = json!({
        "name": "test", "baseUrl": "https://prov.example/v1", "api": "openai-completions",
        "apiKey": "sk-test",
        "compat": { "maxTokensField": "max_tokens", "supportsStore": false },
        "models": [
            { "id": "m1", "compat": { "maxTokensField": "max_completion_tokens" } },
            { "id": "m2", "baseUrl": "https://model.example/v1" }
        ]
    });
    let snap = parse_provider(AppType::Pi, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::Ready);
    // 模型级 compat 覆盖供应商级
    assert_eq!(
        snap.models[0].compat.get("maxTokensField").and_then(|v| v.as_str()),
        Some("max_completion_tokens")
    );
    assert_eq!(snap.models[0].compat.get("supportsStore"), Some(&json!(false)));
    // 模型级 baseUrl 覆盖
    assert_eq!(snap.models[1].base_url_override.as_deref(), Some("https://model.example/v1"));
    assert!(snap.models[0].base_url_override.is_none());
}

#[test]
fn pi_bedrock_unsupported_but_recognized() {
    let sc = json!({
        "name": "test", "baseUrl": "https://x.example", "api": "bedrock-converse-stream",
        "apiKey": "sk-test", "models": [{ "id": "m1" }]
    });
    let snap = parse_provider(AppType::Pi, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::UnsupportedProtocol);
    assert_eq!(snap.protocol, cc_switch_model_tester_lib::domain::ApiProtocol::BedrockConverseStream);
    assert!(!snap.models.is_empty(), "模型仍应展示");
}

#[test]
fn claude_unknown_protocol_is_config_error() {
    let sc = json!({
        "env": { "ANTHROPIC_BASE_URL": "https://x", "ANTHROPIC_MODEL": "m",
                 "ANTHROPIC_AUTH_TOKEN": "t" },
        "api_format": "mystery"
    });
    let snap = parse_provider(AppType::Claude, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::ConfigError);
    assert!(snap.config_error.unwrap().contains("未知的 Claude 协议值"));
}

#[test]
fn claude_legacy_openrouter_compat_mode() {
    let sc = json!({
        "env": { "ANTHROPIC_BASE_URL": "https://x", "ANTHROPIC_MODEL": "m",
                 "ANTHROPIC_AUTH_TOKEN": "t" },
        "openrouter_compat_mode": true
    });
    let snap = parse_provider(AppType::Claude, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert_eq!(snap.protocol, cc_switch_model_tester_lib::domain::ApiProtocol::OpenAiChat);
}

#[test]
fn claude_model_id_markers_stripped() {
    let sc = json!({
        "env": { "ANTHROPIC_BASE_URL": "https://x", "ANTHROPIC_MODEL": "claude-x[1M]",
                 "ANTHROPIC_AUTH_TOKEN": "t" }
    });
    let snap = parse_provider(AppType::Claude, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert_eq!(snap.models[0].model_id, "claude-x", "标记必须从 ID 剥离");
    assert_eq!(snap.models[0].id_markers, vec!["1M"]);
}

#[test]
fn claude_same_id_merges_markers_across_env_keys() {
    // HAIKU 无标记在前、SONNET 带 [1M] 在后：同 id 去重必须合并 markers，不能丢弃
    let sc = json!({
        "env": { "ANTHROPIC_BASE_URL": "https://x", "ANTHROPIC_AUTH_TOKEN": "t",
                 "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-x",
                 "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-x[1M]" }
    });
    let snap = parse_provider(AppType::Claude, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert_eq!(snap.models.len(), 1, "同 id 模型只保留一条");
    assert_eq!(snap.models[0].model_id, "claude-x");
    assert_eq!(snap.models[0].id_markers, vec!["1M"], "后出现的 [1M] 标记必须被合并保留");
}

#[test]
fn claude_managed_by_base_url() {
    let sc = json!({
        "env": { "ANTHROPIC_BASE_URL": "https://api.githubcopilot.com", "ANTHROPIC_MODEL": "m",
                 "ANTHROPIC_API_KEY": "k" }
    });
    let snap = parse_provider(AppType::Claude, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::ManagedAuthSkipped);
}

#[test]
fn codex_managed_by_provider_type() {
    let sc = json!({
        "auth": { "OPENAI_API_KEY": "sk-test" },
        "config": "model_provider = \"custom\"\nmodel = \"m\"\n[model_providers.custom]\nbase_url = \"https://x\"\n"
    });
    let meta = json!({ "providerType": "codex_oauth" });
    let snap = parse_provider(
        AppType::Codex, "p", "P", &sc.to_string(), &meta.to_string(), Vec::new(),
    );
    assert_eq!(snap.status, ProviderStatus::ManagedAuthSkipped);
}

#[test]
fn codex_tokens_are_not_credentials() {
    // 只有 auth.tokens（ChatGPT 登录）而无 API Key：按托管跳过
    let sc = json!({
        "auth": { "tokens": { "access_token": "xxx" } },
        "config": "model_provider = \"custom\"\nmodel = \"m\"\n[model_providers.custom]\nbase_url = \"https://x\"\n"
    });
    let snap = parse_provider(AppType::Codex, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::ManagedAuthSkipped);
}

#[test]
fn codex_env_http_headers_and_wire_api_warnings() {
    let sc = json!({
        "auth": { "OPENAI_API_KEY": "sk-test" },
        "config": "model_provider = \"custom\"\nmodel = \"m\"\n[model_providers.custom]\nbase_url = \"https://x\"\nwire_api = \"chat\"\nenv_http_headers = { \"X-A\" = \"ENV_A\" }\n"
    });
    let snap = parse_provider(AppType::Codex, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::Ready);
    assert!(snap.warnings.iter().any(|w| w.contains("wire_api")));
    assert!(snap.warnings.iter().any(|w| w.contains("env_http_headers")));
}

#[test]
fn claude_invalid_json_config_error() {
    let snap = parse_provider(AppType::Claude, "p", "P", "not-json{", "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::ConfigError);
    assert!(snap.config_error.unwrap().contains("JSON 无效"));
}

#[test]
fn pi_missing_models_is_config_error() {
    let sc = json!({ "name": "t", "baseUrl": "https://x", "api": "openai-completions", "apiKey": "k" });
    let snap = parse_provider(AppType::Pi, "p", "P", &sc.to_string(), "{}", Vec::new());
    assert_eq!(snap.status, ProviderStatus::ConfigError);
    assert!(snap.config_error.unwrap().contains("模型列表为空"));
}

#[test]
fn catalog_view_has_no_secrets() {
    // 视图脱敏验证：endpoint query 参数必须被脱敏
    let sc = json!({
        "env": { "ANTHROPIC_BASE_URL": "https://x.example/v1?api_key=sk-secret123",
                 "ANTHROPIC_MODEL": "m", "ANTHROPIC_AUTH_TOKEN": "t" }
    });
    let snap = parse_provider(AppType::Claude, "p", "P", &sc.to_string(), "{}", Vec::new());
    let view_json = serde_json::to_string(&snap.to_catalog_view()).unwrap();
    assert!(!view_json.contains("sk-secret123"), "视图不得包含明文密钥");
    assert!(view_json.contains("api_key=***"));
    // 原文哈希稳定
    assert_eq!(snap.raw_config_hash.len(), 64);
}
