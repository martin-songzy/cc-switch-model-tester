//! 工具自身本地存储（与 cc-switch 数据库完全独立）。
//!
//! 数据目录：`%LOCALAPPDATA%\CcSwitchModelTester\`
//! 数据库：`data.db`（可写），日志：`logs\`

use std::path::PathBuf;

use rusqlite::Connection;

use crate::error::{AppError, AppErrorKind};

const SCHEMA_SQL: &str = include_str!("store_schema.sql");

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CcSwitchModelTester")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn db_path() -> PathBuf {
    data_dir().join("data.db")
}

/// 默认代理（需求文档 1.5）。调度时：设置值为空 → 使用此默认；“direct”→ 直连。
pub const DEFAULT_PROXY: &str = "socks5://127.0.0.1:1080";

/// 初始化本地存储：建目录、打开连接、建表、写入内置数据。
pub fn init_store() -> Result<Connection, AppError> {
    std::fs::create_dir_all(data_dir())?;
    let conn = Connection::open(db_path())?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    init_connection(&conn)?;
    Ok(conn)
}

/// 在给定连接上执行建表与种子数据（测试可用内存库调用）。
pub fn init_connection(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(SCHEMA_SQL)?;
    migrate(conn)?;
    seed_builtin_prompts(conn)?;
    seed_builtin_negative_rules(conn)?;
    Ok(())
}

/// 老库迁移：给已有 test_attempts 表补上后加的列（CREATE TABLE IF NOT EXISTS 不会改老表）。
fn migrate(conn: &Connection) -> Result<(), AppError> {
    let has_col = conn
        .prepare("PRAGMA table_info(test_attempts)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .any(|c| c.as_deref() == Ok("credential_hint"));
    if !has_col {
        conn.execute("ALTER TABLE test_attempts ADD COLUMN credential_hint TEXT", [])?;
    }
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 内置测试提示词：超短、低成本、秒回（用户要求：回复很短，如“不要思考，直接回复pong”）。
/// id 固定以便「恢复默认」；启动时 REPLACE 更新内置项内容。
const BUILTIN_PROMPTS: [(&str, &str, &str); 8] = [
    (
        "builtin-prompt-01",
        "直接回复pong",
        "不要思考，直接回复pong",
    ),
    (
        "builtin-prompt-02",
        "回复数字42",
        "直接回复数字42，不要任何其他内容",
    ),
    (
        "builtin-prompt-03",
        "回复一个字",
        "只回复一个字：好",
    ),
    (
        "builtin-prompt-04",
        "回复OK",
        "不要解释，直接回复OK",
    ),
    (
        "builtin-prompt-05",
        "回复yes",
        "回复一个英文单词yes，不要其他内容",
    ),
    (
        "builtin-prompt-06",
        "回复1加1结果",
        "直接回复1+1的结果，只要数字",
    ),
    (
        "builtin-prompt-07",
        "回复收到",
        "只回复两个字：收到",
    ),
    (
        "builtin-prompt-08",
        "回复1加2结果",
        "不要思考过程，直接回答1+2等于几，只回复数字",
    ),
];

fn seed_builtin_prompts(conn: &Connection) -> Result<(), AppError> {
    let ts = now_unix();
    for (id, name, content) in BUILTIN_PROMPTS {
        // REPLACE：升级时更新内置项内容（旧版本的长提示词同步替换为短提示词）。
        // 用户自定义提示词（非 builtin id）不受影响。
        conn.execute(
            "INSERT INTO prompt_templates (id, name, content, enabled, is_builtin, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, 1, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, content = excluded.content, is_builtin = 1",
            rusqlite::params![id, name, content, ts],
        )?;
    }
    Ok(())
}

/// 内置负面规则（DevelopmentPlan.md 11.5）。
const BUILTIN_NEGATIVE_RULES: [(&str, &str); 8] = [
    ("builtin-rule-01", "api error"),
    ("builtin-rule-02", "internal server error"),
    ("builtin-rule-03", "invalid api key"),
    ("builtin-rule-04", "authentication failed"),
    ("builtin-rule-05", "model not found"),
    ("builtin-rule-06", "quota exceeded"),
    ("builtin-rule-07", "insufficient quota"),
    ("builtin-rule-08", "request rejected"),
];

fn seed_builtin_negative_rules(conn: &Connection) -> Result<(), AppError> {
    let ts = now_unix();
    for (id, pattern) in BUILTIN_NEGATIVE_RULES {
        conn.execute(
            "INSERT OR IGNORE INTO negative_rules (id, pattern, match_type, scope, case_sensitive, enabled, created_at, updated_at)
             VALUES (?1, ?2, 'contains', 'raw_response', 0, 1, ?3, ?3)",
            rusqlite::params![id, pattern, ts],
        )?;
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    let mut stmt = conn.prepare("SELECT value FROM app_settings WHERE key = ?1")?;
    let mut rows = stmt.query(rusqlite::params![key])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

// ==================== 提示词 / 负面规则管理（M5） ====================

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRow {
    pub id: String,
    pub name: String,
    pub content: String,
    pub enabled: bool,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleRow {
    pub id: String,
    pub pattern: String,
    pub enabled: bool,
    pub is_builtin: bool,
}

pub fn list_prompts(conn: &Connection) -> Result<Vec<PromptRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, content, enabled, is_builtin FROM prompt_templates
         ORDER BY is_builtin DESC, id ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(PromptRow {
                id: r.get(0)?,
                name: r.get(1)?,
                content: r.get(2)?,
                enabled: r.get::<_, i64>(3)? != 0,
                is_builtin: r.get::<_, i64>(4)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 保存提示词：id 为空则新增（随机 id），否则更新。内置项可改文字/开关。
pub fn save_prompt(
    conn: &Connection,
    id: Option<&str>,
    name: &str,
    content: &str,
    enabled: bool,
) -> Result<String, AppError> {
    let name = name.trim();
    let content = content.trim();
    if content.is_empty() {
        return Err(AppError::new(
            AppErrorKind::Validation,
            "提示词内容不能为空",
        ));
    }
    let ts = now_unix();
    match id {
        Some(existing) if !existing.trim().is_empty() => {
            conn.execute(
                "UPDATE prompt_templates SET name = ?2, content = ?3, enabled = ?4, updated_at = ?5
                 WHERE id = ?1",
                rusqlite::params![existing, name, content, enabled as i64, ts],
            )?;
            Ok(existing.to_string())
        }
        _ => {
            let new_id = format!("custom-prompt-{}", ts);
            conn.execute(
                "INSERT INTO prompt_templates (id, name, content, enabled, is_builtin, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)",
                rusqlite::params![new_id, name, content, enabled as i64, ts],
            )?;
            Ok(new_id)
        }
    }
}

/// 删除提示词：内置项不允许删除。
pub fn delete_prompt(conn: &Connection, id: &str) -> Result<(), AppError> {
    let builtin: i64 = conn
        .query_row(
            "SELECT is_builtin FROM prompt_templates WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .map_err(|_| {
            AppError::new(AppErrorKind::Validation, "提示词不存在")
        })?;
    if builtin != 0 {
        return Err(AppError::new(
            AppErrorKind::Validation,
            "内置提示词不能删除，只能停用或修改文字",
        ));
    }
    conn.execute("DELETE FROM prompt_templates WHERE id = ?1", rusqlite::params![id])?;
    Ok(())
}

pub fn list_rules(conn: &Connection) -> Result<Vec<RuleRow>, AppError> {
    // negative_rules 表无 is_builtin 列：内置规则靠固定 id 前缀（builtin-）识别，
    // 自定义规则 id 以 custom- 开头，不会冲突（避免老库 ALTER TABLE 迁移）。
    let mut stmt = conn.prepare(
        "SELECT id, pattern, enabled, CASE WHEN id LIKE 'builtin-%' THEN 1 ELSE 0 END
         FROM negative_rules ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(RuleRow {
                id: r.get(0)?,
                pattern: r.get(1)?,
                enabled: r.get::<_, i64>(2)? != 0,
                is_builtin: r.get::<_, i64>(3)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 保存负面规则（新增时 match_type/scope/case_sensitive 用内置默认）。
pub fn save_rule(
    conn: &Connection,
    id: Option<&str>,
    pattern: &str,
    enabled: bool,
) -> Result<String, AppError> {
    let pattern = pattern.trim().to_lowercase();
    if pattern.is_empty() {
        return Err(AppError::new(
            AppErrorKind::Validation,
            "负面规则不能为空",
        ));
    }
    let ts = now_unix();
    match id {
        Some(existing) if !existing.trim().is_empty() => {
            conn.execute(
                "UPDATE negative_rules SET pattern = ?2, enabled = ?3, updated_at = ?4 WHERE id = ?1",
                rusqlite::params![existing, pattern, enabled as i64, ts],
            )?;
            Ok(existing.to_string())
        }
        _ => {
            let new_id = format!("custom-rule-{}", ts);
            conn.execute(
                "INSERT INTO negative_rules (id, pattern, match_type, scope, case_sensitive, enabled, created_at, updated_at)
                 VALUES (?1, ?2, 'contains', 'raw_response', 0, ?3, ?4, ?4)",
                rusqlite::params![new_id, pattern, enabled as i64, ts],
            )?;
            Ok(new_id)
        }
    }
}

/// 删除负面规则：内置项（id 以 builtin- 开头）不允许删除。
pub fn delete_rule(conn: &Connection, id: &str) -> Result<(), AppError> {
    if id.starts_with("builtin-") {
        return Err(AppError::new(
            AppErrorKind::Validation,
            "内置规则不能删除，只能停用",
        ));
    }
    let n = conn.execute("DELETE FROM negative_rules WHERE id = ?1", rusqlite::params![id])?;
    if n == 0 {
        return Err(AppError::new(AppErrorKind::Validation, "负面规则不存在"));
    }
    Ok(())
}

// ==================== 测试历史（M5） ====================

/// 自动保留时长（秒）：只保留结束时间在最近 15 分钟内的轮次（用户设定）。
pub const HISTORY_KEEP_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRunRow {
    pub run_id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: String,
    pub app_type: String,
    pub total_attempts: i64,
    pub completed_attempts: i64,
    pub passed: i64,
    pub failed: i64,
}

/// 历史轮次列表（含通过/失败统计；cancelled 不计入失败）。先删除过期轮次。
pub fn history_runs(conn: &Connection, now: i64) -> Result<Vec<HistoryRunRow>, AppError> {
    prune_history_before(conn, now - HISTORY_KEEP_SECONDS)?;
    let mut stmt = conn.prepare(
        "SELECT r.run_id, r.started_at, r.finished_at, r.status,
            COALESCE(MIN(a.app_type), '') AS app_type,
            r.total_attempts, r.completed_attempts,
            COALESCE(SUM(CASE WHEN a.status = 'success' THEN 1 ELSE 0 END), 0) AS passed,
            COALESCE(SUM(CASE WHEN a.status = 'failed' AND a.category != 'cancelled' THEN 1 ELSE 0 END), 0) AS failed
         FROM test_runs r
         LEFT JOIN test_attempts a ON a.run_id = r.run_id
         GROUP BY r.run_id
         ORDER BY r.started_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(HistoryRunRow {
                run_id: r.get(0)?,
                started_at: r.get(1)?,
                finished_at: r.get(2)?,
                status: r.get(3)?,
                app_type: r.get(4)?,
                total_attempts: r.get(5)?,
                completed_attempts: r.get(6)?,
                passed: r.get(7)?,
                failed: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 历史明细：形状与 AttemptFinishedView 对齐（camelCase）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryAttemptRow {
    pub run_id: String,
    pub app_type: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_display_name: Option<String>,
    pub protocol: String,
    pub mode: String,
    pub endpoint_display: Option<String>,
    pub attempt_no: i64,
    pub prompt_text: String,
    pub tested_at: i64,
    pub status: String,
    pub category: String,
    pub http_status: Option<i64>,
    pub first_byte_ms: Option<i64>,
    pub total_latency_ms: Option<i64>,
    pub response_chars: i64,
    pub response_summary: Option<String>,
    pub error_summary: Option<String>,
    pub matched_rule: Option<String>,
    pub credential_hint: Option<String>,
    pub duplicate_source_count: i64,
}

pub fn history_attempts(conn: &Connection, run_id: &str) -> Result<Vec<HistoryAttemptRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT run_id, app_type, provider_id, provider_name, model_id, model_display_name,
            protocol, mode, endpoint_display, attempt_no, prompt_text, tested_at,
            status, category, http_status, first_byte_ms, total_latency_ms,
            response_chars, response_summary, error_summary, matched_rule, credential_hint, duplicate_source_count
         FROM test_attempts WHERE run_id = ?1 ORDER BY provider_name, model_id, attempt_no",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![run_id], |r| {
            Ok(HistoryAttemptRow {
                run_id: r.get(0)?,
                app_type: r.get(1)?,
                provider_id: r.get(2)?,
                provider_name: r.get(3)?,
                model_id: r.get(4)?,
                model_display_name: r.get(5)?,
                protocol: r.get(6)?,
                mode: r.get(7)?,
                endpoint_display: r.get(8)?,
                attempt_no: r.get(9)?,
                prompt_text: r.get(10)?,
                tested_at: r.get(11)?,
                status: r.get(12)?,
                category: r.get(13)?,
                http_status: r.get(14)?,
                first_byte_ms: r.get(15)?,
                total_latency_ms: r.get(16)?,
                response_chars: r.get(17)?,
                response_summary: r.get(18)?,
                error_summary: r.get(19)?,
                matched_rule: r.get(20)?,
                credential_hint: r.get(21)?,
                duplicate_source_count: r.get(22)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 删除单轮历史（级联删除明细）。
pub fn delete_history_run(conn: &Connection, run_id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM test_runs WHERE run_id = ?1", rusqlite::params![run_id])?;
    Ok(())
}

/// 清空全部历史。
pub fn clear_history(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM test_runs", [])?;
    Ok(())
}

/// 删除结束时间早于 cutoff 的轮次（级联删除明细）。返回删除数。
pub fn prune_history_before(conn: &Connection, cutoff: i64) -> Result<usize, AppError> {
    let n = conn.execute(
        "DELETE FROM test_runs WHERE COALESCE(finished_at, started_at) < ?1",
        rusqlite::params![cutoff],
    )?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_and_seed_initializes() {
        let conn = Connection::open_in_memory().unwrap();
        init_connection(&conn).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM prompt_templates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 8, "应写入 8 条内置提示词");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM negative_rules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 8, "应写入 8 条内置负面规则");
    }

    #[test]
    fn settings_roundtrip_and_upsert() {
        let conn = Connection::open_in_memory().unwrap();
        init_connection(&conn).unwrap();
        set_setting(&conn, "k", "v1").unwrap();
        set_setting(&conn, "k", "v2").unwrap();
        assert_eq!(get_setting(&conn, "k").unwrap(), Some("v2".to_string()));
        assert_eq!(get_setting(&conn, "missing").unwrap(), None);
    }

    #[test]
    fn prompt_crud_and_builtin_protection() {
        let conn = Connection::open_in_memory().unwrap();
        init_connection(&conn).unwrap();

        // 全量列出：8 条内置
        let all = list_prompts(&conn).unwrap();
        assert_eq!(all.len(), 8);
        assert!(all.iter().all(|p| p.is_builtin));

        // 新增自定义
        let new_id = save_prompt(&conn, None, "测试", "自定义内容", true).unwrap();
        let all = list_prompts(&conn).unwrap();
        assert_eq!(all.len(), 9);
        let custom = all.iter().find(|p| p.id == new_id).unwrap();
        assert!(!custom.is_builtin);

        // 更新自定义（含禁用）
        save_prompt(&conn, Some(&new_id), "改名", "改内容", false).unwrap();
        let all = list_prompts(&conn).unwrap();
        let custom = all.iter().find(|p| p.id == new_id).unwrap();
        assert_eq!(custom.content, "改内容");
        assert!(!custom.enabled);

        // 内置可更新，但不可删除
        save_prompt(&conn, Some("builtin-prompt-01"), "改名", "改内容", true).unwrap();
        assert!(delete_prompt(&conn, "builtin-prompt-01").is_err());
        // 自定义可删除
        delete_prompt(&conn, &new_id).unwrap();
        assert_eq!(list_prompts(&conn).unwrap().len(), 8);

        // 空内容拒绝
        assert!(save_prompt(&conn, None, "x", "   ", true).is_err());
    }

    #[test]
    fn rule_crud_and_builtin_protection() {
        let conn = Connection::open_in_memory().unwrap();
        init_connection(&conn).unwrap();

        let all = list_rules(&conn).unwrap();
        assert_eq!(all.len(), 8);

        let new_id = save_rule(&conn, None, "  Rate Limit ", true).unwrap();
        let all = list_rules(&conn).unwrap();
        assert_eq!(all.len(), 9);
        // 规则统一小写匹配
        let custom = all.iter().find(|r| r.id == new_id).unwrap();
        assert_eq!(custom.pattern, "rate limit");

        assert!(delete_rule(&conn, "builtin-rule-01").is_err());
        delete_rule(&conn, &new_id).unwrap();
        assert_eq!(list_rules(&conn).unwrap().len(), 8);
    }

    #[test]
    fn history_prune_keeps_recent_by_time() {
        let conn = Connection::open_in_memory().unwrap();
        init_connection(&conn).unwrap();

        // 插入 5 轮：结束时间 800/900/1000/1100/1200（cutoff = now-900 = 1000）
        for i in 1..=5i64 {
            let t = 700 + i * 100; // 800..1200
            conn.execute(
                "INSERT INTO test_runs (run_id, started_at, finished_at, status, total_attempts, completed_attempts,
                    original_target_count, deduplicated_target_count, original_attempt_count,
                    deduplicated_attempt_count, config_snapshot_hash, created_at)
                 VALUES (?1, ?2, ?2, 'completed', 1, 1, 1, 1, 1, 1, 'x', ?2)",
                rusqlite::params![format!("run-{i}"), t],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO test_attempts (run_id, app_type, provider_id, provider_name, model_id,
                    protocol, mode, endpoint_url, endpoint_url_hash, dedup_key_hash,
                    attempt_no, prompt_text, tested_at, status, category, credential_hint)
                 VALUES (?1, ?2, 'p', 'P', 'm', 'anthropic', 'non_streaming', 'u', 'h', 'k',
                    1, 'hi', ?3, 'success', 'passed', '…abcd')",
                rusqlite::params![format!("run-{i}"), if i % 2 == 0 { "codex" } else { "claude" }, t],
            )
            .unwrap();
        }

        // now = 1900 → cutoff = 1000：保留 finished_at >= 1000 的 run-3/4/5
        let runs = history_runs(&conn, 1900).unwrap();
        assert_eq!(runs.len(), 3, "应只保留最近 15 分钟内的轮次");
        assert_eq!(runs[0].run_id, "run-5");
        assert_eq!(runs[2].run_id, "run-3");
        // 统计聚合正确
        assert_eq!(runs[0].passed, 1);
        assert_eq!(runs[0].failed, 0);
        // app_type 聚合正确（run-5 是 odd → claude；run-4 是 even → codex）
        assert_eq!(runs[0].app_type, "claude");
        assert_eq!(runs[1].app_type, "codex");

        // 明细查询（含凭据提示）
        let rows = history_attempts(&conn, "run-5").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "success");
        assert_eq!(rows[0].credential_hint.as_deref(), Some("…abcd"));

        // 单轮删除 + 清空
        delete_history_run(&conn, "run-5").unwrap();
        assert_eq!(history_runs(&conn, 1900).unwrap().len(), 2);
        clear_history(&conn).unwrap();
        assert!(history_runs(&conn, 1900).unwrap().is_empty());
    }

    #[test]
    fn migration_adds_credential_hint_column() {
        // 模拟老库：建表后去掉 credential_hint 列，再跑迁移应补回
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.execute("ALTER TABLE test_attempts DROP COLUMN credential_hint", []).unwrap();
        migrate(&conn).unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(test_attempts)")
            .unwrap()
            .query_map([], |r| r.get(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(cols.iter().any(|c| c == "credential_hint"), "迁移应补回 credential_hint 列");
    }
}
