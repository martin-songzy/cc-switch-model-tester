//! Codex 供应商解析（DevelopmentPlan.md 6.2）。
//!
//! settings_config 形如：
//! ```json
//! { "auth": { "OPENAI_API_KEY": "sk-..." },
//!   "config": "model_provider = \"custom\"\nmodel = \"...\"\n[model_providers.custom]\n...",
//!   "modelCatalog": { "models": [ { "model": "id", "displayName": "..." } ] } }
//! ```
//! `config` 是 TOML 文本，必须用完整 TOML 解析器，禁止正则替代。

use serde_json::Value;

use crate::domain::{
    ApiProtocol, AppType, CredentialKind, CredentialValue, ModelSnapshot, ProviderSnapshot,
    ProviderStatus,
};

use super::{detect_managed, error_snapshot, nonempty_str};

const MAX_EXTERNAL_CATALOG_BYTES: u64 = 32 * 1024 * 1024; // 32 MiB

/// toml::Value 取非空字符串（与 serde_json 版本区分）。
fn toml_str<'a>(v: Option<&'a toml::Value>) -> Option<&'a str> {
    v.and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty())
}

/// 收集 TOML 表为 Header 列表。
fn toml_headers(v: Option<&toml::Value>) -> Vec<(String, String)> {
    let Some(t) = v.and_then(|x| x.as_table()) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = t
        .iter()
        .filter_map(|(k, val)| val.as_str().map(|s| (k.to_string(), s.to_string())))
        .collect();
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

pub(super) fn parse(
    provider_id: &str,
    provider_name: &str,
    sc: &Value,
    meta: &Value,
    candidate_endpoints: Vec<String>,
    raw_hash: String,
) -> ProviderSnapshot {
    let mut warnings: Vec<String> = Vec::new();

    // ---- config TOML ----
    let config_text = sc.get("config").and_then(|v| v.as_str()).unwrap_or("");
    let toml_val: toml::Value = if config_text.trim().is_empty() {
        toml::Value::Table(Default::default())
    } else {
        match toml::from_str(config_text) {
            Ok(v) => v,
            Err(e) => {
                return error_snapshot(
                    AppType::Codex,
                    provider_id,
                    provider_name,
                    raw_hash,
                    candidate_endpoints,
                    format!("config TOML 解析失败: {e}"),
                )
            }
        }
    };

    // ---- 托管认证检测 ----
    let provider_table = current_provider_table(&toml_val);
    let base_url_raw = provider_table
        .and_then(|t| toml_str(t.get("base_url")))
        .or_else(|| toml_str(toml_val.get("base_url")))
        .map(|s| s.trim().to_string());
    if detect_managed(meta, base_url_raw.as_deref()) {
        return managed_snapshot(provider_id, provider_name, raw_hash, candidate_endpoints);
    }

    // ---- 协议：meta.apiFormat；缺省按 openai_responses 并警告 ----
    let protocol = match nonempty_str(meta.get("apiFormat")) {
        Some("openai_responses") => ApiProtocol::OpenAiResponses,
        Some("openai_chat") => ApiProtocol::OpenAiChat,
        Some("anthropic") => ApiProtocol::AnthropicMessages,
        Some(other) => {
            return error_snapshot(
                AppType::Codex,
                provider_id,
                provider_name,
                raw_hash,
                candidate_endpoints,
                format!("未知的 Codex 协议值 \"{other}\"（合法值: openai_responses/openai_chat/anthropic）"),
            )
        }
        None => {
            warnings.push(
                "meta.apiFormat 未设置，按 Codex 默认 openai_responses 处理".to_string(),
            );
            ApiProtocol::OpenAiResponses
        }
    };

    // ---- Base URL：仅当前 provider 表；缺失才回退顶层 ----
    let Some(endpoint) = base_url_raw else {
        return error_snapshot(
            AppType::Codex,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            "缺少 Base URL（当前 model_provider 表与顶层均无 base_url）".to_string(),
        );
    };

    // ---- 客户端 wire_api 与上游协议是两回事（文档 6.2）----
    if let Some(wire) = toml_str(provider_table.and_then(|t| t.get("wire_api"))) {
        if wire != "responses" {
            warnings.push(format!(
                "客户端 wire_api = \"{wire}\"（非 responses）；本工具按 meta.apiFormat 选择上游协议，不受其影响"
            ));
        }
    }

    // ---- env_http_headers：不读取环境变量 ----
    if provider_table
        .and_then(|t| t.get("env_http_headers"))
        .and_then(|v| v.as_table())
        .is_some_and(|t| !t.is_empty())
    {
        warnings.push(
            "配置含 env_http_headers（依赖系统环境变量），无法按 cc-switch 数据库独立重现，已忽略"
                .to_string(),
        );
    }

    // ---- 模型集合：DB modelCatalog.models 优先 → config.model → 外部 model_catalog_json ----
    let mut models: Vec<ModelSnapshot> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(catalog_models) = sc
        .get("modelCatalog")
        .and_then(|c| c.get("models"))
        .and_then(|v| v.as_array())
    {
        for m in catalog_models {
            // DB SSOT 简化条目：model / displayName
            let Some(id) = nonempty_str(m.get("model")) else {
                continue;
            };
            let (id, markers) = crate::domain::strip_model_id_markers(id);
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            let mut snap = super::model_with_markers(id, markers);
            snap.display_name = nonempty_str(m.get("displayName")).map(|s| s.to_string());
            models.push(snap);
        }
    }

    if models.is_empty() {
        if let Some(id) = toml_str(toml_val.get("model")) {
            let (id, markers) = crate::domain::strip_model_id_markers(id);
            if !id.is_empty() && seen.insert(id.clone()) {
                models.push(super::model_with_markers(id, markers));
            }
        }
    }

    if models.is_empty() {
        // 外部 model_catalog_json（仅本地文件，不覆盖 DB 语义）
        if let Some(catalog_path) = toml_str(toml_val.get("model_catalog_json")) {
            match read_external_catalog(catalog_path) {
                Ok(entries) => {
                    for m in entries {
                        // 外部完整目录条目：slug / display_name（文档 6.2）
                        let Some(slug) = nonempty_str(m.get("slug")) else {
                            continue;
                        };
                        let (id, markers) = crate::domain::strip_model_id_markers(slug);
                        if id.is_empty() || !seen.insert(id.clone()) {
                            continue;
                        }
                        let mut snap = super::model_with_markers(id, markers);
                        snap.display_name = nonempty_str(m.get("display_name")).map(|s| s.to_string());
                        models.push(snap);
                    }
                    if models.is_empty() {
                        warnings.push(format!(
                            "外部模型目录 {catalog_path} 中没有可用的 slug 条目，模型列表为空"
                        ));
                    }
                }
                Err(w) => {
                    warnings.push(w);
                }
            }
        }
    }

    if models.is_empty() {
        return error_snapshot(
            AppType::Codex,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            "模型列表为空（无 modelCatalog.models、无 config.model、无可用外部模型目录）".to_string(),
        );
    }

    // ---- 凭据：auth.OPENAI_API_KEY > 当前 provider 表 experimental_bearer_token ----
    let mut credential: Option<CredentialValue> = None;
    let auth_key = sc
        .get("auth")
        .and_then(|a| a.get("OPENAI_API_KEY"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty());
    if let Some(secret) = auth_key {
        credential = Some(CredentialValue {
            kind: CredentialKind::BearerKey,
            secret: secret.trim().to_string(),
        });
    } else if let Some(token) = provider_table
        .and_then(|t| toml_str(t.get("experimental_bearer_token")))
    {
        credential = Some(CredentialValue {
            kind: CredentialKind::BearerKey,
            secret: token.trim().to_string(),
        });
    }

    // auth.tokens 等托管登录信息不作为凭据
    let has_tokens = sc
        .get("auth")
        .and_then(|a| a.get("tokens"))
        .and_then(|v| v.as_object())
        .is_some_and(|t| !t.is_empty());
    if has_tokens {
        warnings.push(
            "auth.tokens 为 ChatGPT 托管登录信息，不作为本工具凭据".to_string(),
        );
        if credential.is_none() {
            return managed_snapshot(provider_id, provider_name, raw_hash, candidate_endpoints);
        }
    }

    let requires_openai_auth = provider_table
        .and_then(|t| t.get("requires_openai_auth"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if credential.is_none() {
        if requires_openai_auth {
            return error_snapshot(
                AppType::Codex,
                provider_id,
                provider_name,
                raw_hash,
                candidate_endpoints,
                super::missing_credential_error(
                    "requires_openai_auth = true 且无 auth.OPENAI_API_KEY / experimental_bearer_token",
                ),
            );
        }
        return error_snapshot(
            AppType::Codex,
            provider_id,
            provider_name,
            raw_hash,
            candidate_endpoints,
            super::missing_credential_error(
                "无 auth.OPENAI_API_KEY 且当前 provider 表无 experimental_bearer_token",
            ),
        );
    }

    // ---- 字面量自定义 Header ----
    let headers = toml_headers(provider_table.and_then(|t| t.get("http_headers")));

    let full_url = meta.get("isFullUrl").and_then(|v| v.as_bool()).unwrap_or(false);

    ProviderSnapshot {
        app: AppType::Codex,
        provider_id: provider_id.to_string(),
        provider_name: provider_name.to_string(),
        endpoint,
        candidate_endpoints,
        protocol,
        raw_protocol: nonempty_str(meta.get("apiFormat")).map(|s| s.to_string()),
        models,
        credential,
        headers,
        custom_user_agent: nonempty_str(meta.get("customUserAgent")).map(|s| s.to_string()),
        full_url,
        compat: serde_json::Value::Null,
        passthrough: sc.clone(),
        raw_config_hash: raw_hash,
        status: ProviderStatus::Ready,
        warnings,
        config_error: None,
    }
}

/// 取当前 `model_provider` 对应的 `[model_providers.<name>]` 表。
fn current_provider_table(toml_val: &toml::Value) -> Option<&toml::Value> {
    let name = toml_str(toml_val.get("model_provider"))?;
    toml_val
        .get("model_providers")
        .and_then(|mp| mp.get(name))
}

/// 托管认证跳过快照。
fn managed_snapshot(
    provider_id: &str,
    provider_name: &str,
    raw_hash: String,
    candidate_endpoints: Vec<String>,
) -> ProviderSnapshot {
    let mut snap = error_snapshot(
        AppType::Codex,
        provider_id,
        provider_name,
        raw_hash,
        candidate_endpoints,
        String::new(),
    );
    snap.status = ProviderStatus::ManagedAuthSkipped;
    snap.config_error = None;
    snap
}

/// 解析外部模型目录的路径：绝对路径仅当位于 Codex 配置目录内时读取；
/// 相对路径以 Codex 配置目录（默认 `%USERPROFILE%\.codex`）为基准。
/// 返回 models 数组元素；失败返回用户可读警告文本。
pub(super) fn read_external_catalog(raw_path: &str) -> Result<Vec<Value>, String> {
    let base = dirs::home_dir()
        .map(|h| h.join(".codex"))
        .ok_or_else(|| "无法定位 Codex 配置目录".to_string())?;

    let candidate = std::path::PathBuf::from(raw_path);
    let full = if candidate.is_absolute() {
        candidate
    } else {
        base.join(&candidate)
    };

    // 路径安全：canonicalize 后必须仍在 Codex 配置目录内（拒绝目录穿越与符号链接逃逸）
    let canonical = full.canonicalize().map_err(|e| {
        format!("外部模型目录不可读（{raw_path}）: {e}，已回退到 config.model")
    })?;
    let base_canonical = base.canonicalize().map_err(|e| {
        format!("Codex 配置目录不可定位: {e}，已回退到 config.model")
    })?;
    if !canonical.starts_with(&base_canonical) {
        return Err(format!(
            "外部模型目录 {raw_path} 位于 Codex 配置目录之外，出于安全考虑拒绝读取"
        ));
    }

    let meta = std::fs::metadata(&canonical)
        .map_err(|e| format!("外部模型目录不可读（{raw_path}）: {e}"))?;
    if meta.len() > MAX_EXTERNAL_CATALOG_BYTES {
        return Err(format!(
            "外部模型目录超过 32 MiB 上限（{} 字节），拒绝读取",
            meta.len()
        ));
    }

    let text = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("外部模型目录读取失败（{raw_path}）: {e}"))?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| format!("外部模型目录不是合法 JSON（{raw_path}）: {e}"))?;
    let models = v
        .get("models")
        .and_then(|m| m.as_array())
        .ok_or_else(|| format!("外部模型目录缺少 models 数组（{raw_path}）"))?;
    Ok(models.clone())
}
