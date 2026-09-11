//! 可用性判定引擎（DevelopmentPlan.md 第 11 节）。
//!
//! 判定顺序：配置校验 → 网络 → HTTP 状态 → 协议解析 → 内容提取 → 负面规则 → 结论。
//! 单次 attempt 只有成功或具体失败原因；`unstable` 是模型汇总状态（不在此产生）。

use serde::Serialize;

use crate::http::ExecutionOutput;
use crate::redact;

/// 单次尝试的最终判定。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptVerdict {
    pub status: AttemptStatus,
    pub category: AttemptCategory,
    /// HTTP 状态码（网络失败为 None）
    pub http_status: Option<u16>,
    /// 命中的负面规则（negative_match 时有值）
    pub matched_rule: Option<String>,
    /// 用户可读说明
    pub message: String,
    /// 响应摘要（脱敏，≤4096 字符）
    pub response_summary: Option<String>,
    /// 错误摘要（脱敏）
    pub error_summary: Option<String>,
    /// 判定用到的重要字段
    pub finish_signal: bool,
    pub has_text: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    Success,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptCategory {
    Success,
    NetworkError,
    AuthenticationFailed,
    RateLimited,
    Unavailable,
    ConfigurationError,
    ProtocolError,
    EmptyResponse,
    NegativeMatch,
    ResponseTooLarge,
    UnsupportedAuthentication,
    Cancelled,
}

/// 已编译的负面规则（运行时缓存）。
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub id: String,
    pub pattern: String,
    pub regex: Option<regex::Regex>,
    pub case_sensitive: bool,
}

impl CompiledRule {
    pub fn compile(
        id: &str,
        pattern: &str,
        match_type: &str,
        case_sensitive: bool,
    ) -> Result<Self, String> {
        let regex = match match_type {
            "regex" => {
                let mut b = regex::RegexBuilder::new(pattern);
                if !case_sensitive {
                    b.case_insensitive(true);
                }
                Some(b.build().map_err(|e| format!("正则编译失败（规则 {id}）: {e}"))?)
            }
            _ => None,
        };
        Ok(Self {
            id: id.to_string(),
            pattern: pattern.to_string(),
            regex,
            case_sensitive,
        })
    }

    /// 是否命中。匹配范围 = 提取文本 + 协议错误对象（error_fields / extracted_text）。
    fn matches(&self, haystack: &str) -> bool {
        if self.pattern.is_empty() {
            return false;
        }
        match &self.regex {
            Some(re) => re.is_match(haystack),
            None => {
                if self.case_sensitive {
                    haystack.contains(&self.pattern)
                } else {
                    let lower = haystack.to_lowercase();
                    lower.contains(&self.pattern.to_lowercase())
                }
            }
        }
    }
}

/// 从 SQLite 读取启用的负面规则并编译；正则编译失败返回错误（不静默忽略，文档 11.5）。
pub fn load_compiled_rules(conn: &rusqlite::Connection) -> Result<Vec<CompiledRule>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, pattern, match_type, case_sensitive FROM negative_rules WHERE enabled = 1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? != 0,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        let (id, pattern, match_type, case_sensitive) = r.map_err(|e| e.to_string())?;
        out.push(CompiledRule::compile(&id, &pattern, &match_type, case_sensitive)?);
    }
    Ok(out)
}

const SUMMARY_MAX: usize = 4096;

/// 判定一次尝试。`rules` 为已启用负面规则（可空切片）。
pub fn judge(exec: &ExecutionOutput, rules: &[CompiledRule]) -> AttemptVerdict {
    // 1. 用户取消由调度层标记（此处不处理）

    // 2. 网络层失败
    if let Some(err) = &exec.network_error {
        return verdict_failed(
            exec,
            AttemptCategory::NetworkError,
            format!("网络错误: {err}"),
            None,
        );
    }

    let Some(status) = exec.http_status else {
        return verdict_failed(
            exec,
            AttemptCategory::NetworkError,
            "无 HTTP 状态（连接未建立）".to_string(),
            None,
        );
    };

    // 3. HTTP 状态分类（文档 11.3），失败时附带响应原文摘要帮助诊断
    let body_head = exec.parsed.raw_body_head.clone().unwrap_or_default();
    let body_head_note = if body_head.is_empty() {
        String::new()
    } else {
        format!("\n响应: {}", body_head)
    };
    let http_category = match status {
        401 | 403 => Some((AttemptCategory::AuthenticationFailed, format!("供应商返回认证失败（{status}），认证信息可能无效或没有该模型权限{body_head_note}"))),
        429 => Some((AttemptCategory::RateLimited, "请求被限流（429），不自动重试".to_string())),
        404 => Some((AttemptCategory::Unavailable, format!("端点或模型不存在（404）{body_head_note}"))),
        408 => Some((AttemptCategory::NetworkError, "请求超时（408）".to_string())),
        500..=599 => Some((AttemptCategory::Unavailable, format!("上游服务错误（{status}）{body_head_note}"))),
        s if (400..500).contains(&s) => {
            let hint = if body_head.to_lowercase().contains("<!doctype")
                || body_head.to_lowercase().contains("<html")
            {
                "（返回的是网页而非 API 响应，Base URL 可能不正确）"
            } else {
                "（配置可能不正确）"
            };
            Some((AttemptCategory::ConfigurationError, format!("请求被拒绝（{status}）{hint}{body_head_note}")))
        }
        s if (200..300).contains(&s) => None,
        _ => Some((AttemptCategory::Unavailable, format!("非预期的 HTTP 状态（{status}）{body_head_note}"))),
    };
    if let Some((category, msg)) = http_category {
        return verdict_failed(exec, category, msg, None);
    }

    // 4. 2xx：协议解析层结果
    if exec.parsed.aborted {
        return verdict_failed(
            exec,
            AttemptCategory::ResponseTooLarge,
            exec.parsed.parse_error.clone().unwrap_or_else(|| "响应超限".to_string()),
            None,
        );
    }
    if let Some(pe) = &exec.parsed.parse_error {
        return verdict_failed(
            exec,
            AttemptCategory::ProtocolError,
            format!("协议解析失败: {pe}"),
            None,
        );
    }
    // 流式缺少完成信号 = 协议失败（文档 8.6）
    if !exec.parsed.finish_signal {
        return verdict_failed(
            exec,
            AttemptCategory::ProtocolError,
            "流式响应未正常完成（缺少完成信号或中途断开）".to_string(),
            None,
        );
    }

    // 5. 协议级错误对象（200 + error 判失败，文档 11.4）
    let error_text = exec.parsed.error_object.clone().unwrap_or_default();
    if exec.parsed.error_object.is_some() {
        return verdict_failed(
            exec,
            AttemptCategory::ProtocolError,
            format!("供应商返回错误对象: {error_text}"),
            None,
        );
    }

    // 6. 空输出
    if !exec.parsed.has_text() {
        return verdict_failed(
            exec,
            AttemptCategory::EmptyResponse,
            "模型返回空内容（HTTP 200 但无有效文本）".to_string(),
            None,
        );
    }

    // 7. 负面规则（对提取文本 + 错误摘要匹配）
    let haystack = format!("{}\n{}", exec.parsed.text, error_text);
    for rule in rules {
        if rule.matches(&haystack) {
            return verdict_failed(
                exec,
                AttemptCategory::NegativeMatch,
                format!("命中负面规则 \"{}\"", rule.pattern),
                Some(rule.id.clone()),
            );
        }
    }

    // 8. 成功
    AttemptVerdict {
        status: AttemptStatus::Success,
        category: AttemptCategory::Success,
        http_status: exec.http_status,
        matched_rule: None,
        message: "成功".to_string(),
        response_summary: Some(redact::redact_text(&truncate(&exec.parsed.text, SUMMARY_MAX))),
        error_summary: None,
        finish_signal: exec.parsed.finish_signal,
        has_text: exec.parsed.has_text(),
    }
}

fn verdict_failed(
    exec: &ExecutionOutput,
    category: AttemptCategory,
    message: String,
    matched_rule: Option<String>,
) -> AttemptVerdict {
    let response_summary = if exec.parsed.text.is_empty() {
        None
    } else {
        Some(redact::redact_text(&truncate(&exec.parsed.text, SUMMARY_MAX)))
    };
    // 错误摘要优先级：网络错误 > 响应原文开头（供应商实际返回了什么，最有诊断价值）
    // > 协议错误对象 > 解析器错误（“JSON 解析失败”这类无信息量的排在最后）
    let error_summary = exec
        .network_error
        .clone()
        .or_else(|| exec.parsed.raw_body_head.clone())
        .or(exec.parsed.error_object.clone())
        .or(exec.parsed.parse_error.clone())
        .map(|s| redact::redact_text(&truncate(&s, 1024)));
    AttemptVerdict {
        status: AttemptStatus::Failed,
        category,
        http_status: exec.http_status,
        matched_rule,
        message,
        response_summary,
        error_summary,
        finish_signal: exec.parsed.finish_signal,
        has_text: exec.parsed.has_text(),
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{cut}…[已截断]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ParsedResponse;

    fn exec_ok(text: &str) -> ExecutionOutput {
        ExecutionOutput {
            http_status: Some(200),
            parsed: ParsedResponse {
                text: text.to_string(),
                finish_signal: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn success_path() {
        let v = judge(&exec_ok("正常回答"), &[]);
        assert_eq!(v.status, AttemptStatus::Success);
        assert_eq!(v.category, AttemptCategory::Success);
    }

    #[test]
    fn network_error() {
        let mut e = exec_ok("");
        e.network_error = Some("连接失败".to_string());
        e.http_status = None;
        let v = judge(&e, &[]);
        assert_eq!(v.category, AttemptCategory::NetworkError);
    }

    #[test]
    fn http_classification() {
        for (code, expect) in [
            (401u16, AttemptCategory::AuthenticationFailed),
            (403, AttemptCategory::AuthenticationFailed),
            (429, AttemptCategory::RateLimited),
            (404, AttemptCategory::Unavailable),
            (500, AttemptCategory::Unavailable),
            (503, AttemptCategory::Unavailable),
            (400, AttemptCategory::ConfigurationError),
        ] {
            let mut e = exec_ok("");
            e.http_status = Some(code);
            let v = judge(&e, &[]);
            assert_eq!(v.category, expect, "code={code}");
        }
    }

    #[test]
    fn status200_with_error_object_fails() {
        let mut e = exec_ok("");
        e.parsed.error_object = Some("api error: internal".to_string());
        let v = judge(&e, &[]);
        assert_eq!(v.status, AttemptStatus::Failed);
        assert_eq!(v.category, AttemptCategory::ProtocolError);
    }

    #[test]
    fn status200_empty_fails() {
        let v = judge(&exec_ok(""), &[]);
        assert_eq!(v.category, AttemptCategory::EmptyResponse);
    }

    #[test]
    fn negative_contains_rule() {
        let rules = vec![CompiledRule::compile("r1", "api error", "contains", false).unwrap()];
        let v = judge(&exec_ok("抱歉，API error 发生"), &rules);
        assert_eq!(v.category, AttemptCategory::NegativeMatch);
        assert_eq!(v.matched_rule.as_deref(), Some("r1"));
        // 不误伤
        let v2 = judge(&exec_ok("一切正常"), &rules);
        assert_eq!(v2.status, AttemptStatus::Success);
    }

    #[test]
    fn negative_regex_rule_case_insensitive() {
        let rules = vec![CompiledRule::compile("r2", "model\\s+not\\s+found", "regex", false).unwrap()];
        let v = judge(&exec_ok("Error: MODEL NOT FOUND"), &rules);
        assert_eq!(v.category, AttemptCategory::NegativeMatch);
    }

    #[test]
    fn regex_compile_failure_is_error_not_silent() {
        assert!(CompiledRule::compile("bad", "([unclosed", "regex", false).is_err());
    }

    #[test]
    fn stream_without_finish_signal_fails() {
        let mut e = exec_ok("部分文本");
        e.parsed.finish_signal = false;
        let v = judge(&e, &[]);
        assert_eq!(v.category, AttemptCategory::ProtocolError, "流式断开即使有部分文本也判失败");
    }

    #[test]
    fn oversized_response_fails() {
        let mut e = exec_ok("x");
        e.parsed.aborted = true;
        e.parsed.parse_error = Some("响应超出 8 MiB 上限，已中止读取".to_string());
        let v = judge(&e, &[]);
        assert_eq!(v.category, AttemptCategory::ResponseTooLarge);
    }

    #[test]
    fn summary_is_redacted_and_truncated() {
        let long: String = "sk-abcdefghijklmnop1234 ".repeat(500);
        let v = judge(&exec_ok(&long), &[]);
        let s = v.response_summary.unwrap();
        assert!(s.contains("sk-***"), "摘要必须脱敏");
        assert!(s.chars().count() <= SUMMARY_MAX + 20);
    }
}
