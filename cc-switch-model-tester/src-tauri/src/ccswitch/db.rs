//! cc-switch SQLite 数据库只读访问。
//!
//! 安全约定：
//! - 连接一律只读（OpenFlags READ_ONLY + `PRAGMA query_only`），绝不执行写操作；
//! - `settings_config` 含明文 API Key，只能留在 Rust 内存中，
//!   发往前端的任何结构只允许包含元数据与有效性标记。

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::error::{AppError, AppErrorKind};

/// 支持的 schema 范围：cc-switch 3.19.x（16）至 3.21.x（19）。
/// providers / provider_endpoints 表结构在 16~19 之间一致：
/// 17~19 的差异仅在不使用的表（代理日志/用量/技能等），19 只给 mcp_servers/skills 加 enabled_mcode 列。
pub const SCHEMA_MIN: i32 = 16;
pub const SCHEMA_MAX: i32 = 19;

/// schema 版本越界告警（None = 在已验证范围内）。只告警不拦截：
/// 上游新版本通常只改本工具不用的表，providers 结构兼容时照常读取。
pub fn schema_warning(version: i32) -> Option<String> {
    if version > SCHEMA_MAX {
        Some(format!(
            "cc-switch 数据库版本较新（schema {version}），本工具验证至 schema {SCHEMA_MAX}；通常可正常读取，若出现解析异常请更新本工具"
        ))
    } else if version < SCHEMA_MIN {
        Some(format!(
            "cc-switch 数据库版本较旧（schema {version}），本工具验证自 schema {SCHEMA_MIN}；若出现解析异常请升级 cc-switch"
        ))
    } else {
        None
    }
}

/// 本工具读取的三个应用类型。
pub const APP_TYPES: [&str; 3] = ["claude", "codex", "pi"];

pub fn is_supported_app_type(app_type: &str) -> bool {
    APP_TYPES.contains(&app_type)
}

/// 默认数据库路径：`%USERPROFILE%\.cc-switch\cc-switch.db`
pub fn default_db_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cc-switch").join("cc-switch.db"))
}

/// 打开只读连接。文件不存在 / 非 SQLite / 被锁定分别映射到不同错误分类。
pub fn open_read_only(path: &Path) -> Result<Connection, AppError> {
    if !path.exists() {
        return Err(AppError::new(
            AppErrorKind::CcSwitchDbNotFound,
            format!("数据库文件不存在: {}", path.display()),
        ));
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;
    let conn = Connection::open_with_flags(path, flags).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not a database") || msg.contains("malformed") {
            AppError::new(
                AppErrorKind::CcSwitchDbNotSqlite,
                format!("文件不是合法的 SQLite 数据库: {}", path.display()),
            )
        } else {
            AppError::new(
                AppErrorKind::CcSwitchDbOpenFailed,
                format!(
                    "数据库打开失败（可能正被 cc-switch 独占锁定或无访问权限）: {msg}"
                ),
            )
        }
    })?;
    // 双保险：即使拿到的是可写连接也禁止写
    conn.pragma_update(None, "query_only", "ON")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// 读取 `PRAGMA user_version`。
pub fn read_schema_version(conn: &Connection) -> Result<i32, AppError> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| AppError::new(AppErrorKind::CcSwitchDbOpenFailed, e.to_string()))
}

/// 错误信息的前端表示。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceError {
    pub kind: AppErrorKind,
    pub message: String,
}

impl SourceError {
    fn from_app_error(e: &AppError) -> Self {
        Self {
            kind: e.kind,
            message: e.message.clone(),
        }
    }
}

/// 每个应用类型的供应商数量。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppTypeCount {
    pub app_type: String,
    pub count: i64,
}

/// 数据源探测结果（错误不抛出，附带在 error 字段中）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub path: String,
    pub exists: bool,
    pub schema_version: Option<i32>,
    pub schema_warning: Option<String>,
    pub provider_counts: Vec<AppTypeCount>,
    pub error: Option<SourceError>,
}

/// 探测一个数据库路径：存在性、schema 版本、三类供应商数量。
/// 任何失败都以 `error` 字段返回，不 panic。
pub fn inspect_source(path: &Path) -> SourceInfo {
    let path_str = path.to_string_lossy().into_owned();
    if !path.exists() {
        return SourceInfo {
            path: path_str,
            exists: false,
            schema_version: None,
            schema_warning: None,
            provider_counts: Vec::new(),
            error: Some(SourceError {
                kind: AppErrorKind::CcSwitchDbNotFound,
                message: "数据库文件不存在".to_string(),
            }),
        };
    }
    match open_read_only(path)
        .and_then(|conn| inspect_connection(&conn))
    {
        Ok((version, counts, warning)) => SourceInfo {
            path: path_str,
            exists: true,
            schema_version: Some(version),
            schema_warning: warning,
            provider_counts: counts,
            error: None,
        },
        Err(e) => SourceInfo {
            path: path_str,
            exists: true,
            schema_version: None,
            schema_warning: None,
            provider_counts: Vec::new(),
            error: Some(SourceError::from_app_error(&e)),
        },
    }
}

/// 在已打开的只读连接上读取版本、计数与版本告警（版本越界只告警不拦截）。
pub fn inspect_connection(
    conn: &Connection,
) -> Result<(i32, Vec<AppTypeCount>, Option<String>), AppError> {
    let version = read_schema_version(conn)?;
    let warning = schema_warning(version);
    let counts = query_provider_counts(conn)?;
    Ok((version, counts, warning))
}

/// 统计三类应用各自的供应商数量。
pub fn query_provider_counts(conn: &Connection) -> Result<Vec<AppTypeCount>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT app_type, COUNT(*) AS n
         FROM providers
         WHERE app_type IN ('claude', 'codex', 'pi')
         GROUP BY app_type
         ORDER BY app_type",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(AppTypeCount {
            app_type: row.get(0)?,
            count: row.get(1)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 供应商摘要（发给前端的元数据；不含 settings_config / meta 原文）。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub app_type: String,
    pub name: String,
    pub website_url: Option<String>,
    pub category: Option<String>,
    pub sort_index: Option<i64>,
    pub is_current: bool,
    /// settings_config 是否为合法 JSON（M1 仅做有效性检查）
    pub settings_json_valid: bool,
    /// meta 是否为合法 JSON
    pub meta_json_valid: bool,
}

/// 按 app_type 列出供应商摘要，排序遵循文档 5.2：
/// `ORDER BY app_type, COALESCE(sort_index, 9223372036854775807), name`。
pub fn list_provider_summaries(
    conn: &Connection,
    app_type: &str,
) -> Result<Vec<ProviderSummary>, AppError> {
    if !is_supported_app_type(app_type) {
        return Err(AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            format!("不支持的应用类型: {app_type}"),
        ));
    }
    let mut stmt = conn.prepare(
        "SELECT id, app_type, name, website_url, category, sort_index, is_current, settings_config, meta
         FROM providers
         WHERE app_type = ?1
         ORDER BY COALESCE(sort_index, 9223372036854775807), name",
    )?;
    let rows = stmt.query_map(rusqlite::params![app_type], |row| {
        let settings: String = row.get(7)?;
        let meta: String = row.get(8)?;
        Ok(ProviderSummary {
            id: row.get(0)?,
            app_type: row.get(1)?,
            name: row.get(2)?,
            website_url: row.get(3)?,
            category: row.get(4)?,
            sort_index: row.get(5)?,
            is_current: row.get::<_, i64>(6)? != 0,
            settings_json_valid: serde_json::from_str::<serde_json::Value>(&settings)
                .is_ok(),
            meta_json_valid: serde_json::from_str::<serde_json::Value>(&meta).is_ok(),
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 按 app_type 列出供应商完整行（含 settings_config / meta 原文，仅限 Rust 内存使用，
/// 绝不序列化到前端——settings_config 含明文 API Key）。
pub struct ProviderRowFull {
    pub id: String,
    pub app_type: String,
    pub name: String,
    pub settings_config: String,
    pub meta: String,
    pub category: Option<String>,
    pub website_url: Option<String>,
    pub sort_index: Option<i64>,
    pub is_current: bool,
}

pub fn load_provider_rows(
    conn: &Connection,
    app_type: &str,
) -> Result<Vec<ProviderRowFull>, AppError> {
    if !is_supported_app_type(app_type) {
        return Err(AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            format!("不支持的应用类型: {app_type}"),
        ));
    }
    let mut stmt = conn.prepare(
        "SELECT id, app_type, name, settings_config, meta, category, website_url, sort_index, is_current
         FROM providers
         WHERE app_type = ?1
         ORDER BY COALESCE(sort_index, 9223372036854775807), name",
    )?;
    let rows = stmt.query_map(rusqlite::params![app_type], |row| {
        Ok(ProviderRowFull {
            id: row.get(0)?,
            app_type: row.get(1)?,
            name: row.get(2)?,
            settings_config: row.get(3)?,
            meta: row.get(4)?,
            category: row.get(5)?,
            website_url: row.get(6)?,
            sort_index: row.get(7)?,
            is_current: row.get::<_, i64>(8)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 读取候选端点（provider_endpoints 表），按 provider_id 分组、URL 去重保序。
pub fn load_candidate_endpoints(
    conn: &Connection,
    app_type: &str,
) -> Result<std::collections::HashMap<String, Vec<String>>, AppError> {
    if !is_supported_app_type(app_type) {
        return Err(AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            format!("不支持的应用类型: {app_type}"),
        ));
    }
    let mut stmt = conn.prepare(
        "SELECT provider_id, url FROM provider_endpoints
         WHERE app_type = ?1
         ORDER BY provider_id, added_at, id",
    )?;
    let rows = stmt.query_map(rusqlite::params![app_type], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for r in rows {
        let (pid, url) = r?;
        let urls = map.entry(pid).or_default();
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建立与真实库结构兼容的最小 providers 表。
    fn setup_min_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE providers (
                id TEXT NOT NULL,
                app_type TEXT NOT NULL,
                name TEXT NOT NULL,
                settings_config TEXT NOT NULL,
                website_url TEXT,
                category TEXT,
                created_at INTEGER,
                sort_index INTEGER,
                notes TEXT,
                icon TEXT,
                icon_color TEXT,
                meta TEXT NOT NULL DEFAULT '{}',
                is_current BOOLEAN NOT NULL DEFAULT 0,
                in_failover_queue BOOLEAN NOT NULL DEFAULT 0,
                PRIMARY KEY (id, app_type)
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta, sort_index)
             VALUES ('p1', 'claude', 'Provider A', '{\"env\":{}}', '{}', 2),
                    ('p2', 'claude', 'Provider B', 'not-json', '{}', 1),
                    ('p3', 'pi', 'Pi Provider', '{\"api\":\"anthropic-messages\"}', '{}', 1)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn schema_check_boundaries() {
        assert!(schema_warning(16).is_none());
        assert!(schema_warning(17).is_none());
        assert!(schema_warning(18).is_none());
        assert!(schema_warning(19).is_none());
        assert!(schema_warning(20).unwrap().contains("schema 20"));
        assert!(schema_warning(15).unwrap().contains("schema 15"));
    }

    #[test]
    fn provider_counts_only_three_app_types() {
        let conn = setup_min_db();
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config)
             VALUES ('g1', 'gemini', 'Gemini P', '{}')",
            [],
        )
        .unwrap();
        let counts = query_provider_counts(&conn).unwrap();
        assert_eq!(counts.len(), 2); // claude / pi，gemini 不计
        assert!(counts.iter().any(|c| c.app_type == "claude" && c.count == 2));
        assert!(counts.iter().any(|c| c.app_type == "pi" && c.count == 1));
    }

    #[test]
    fn summaries_flag_invalid_json_and_sort_by_sort_index() {
        let conn = setup_min_db();
        let list = list_provider_summaries(&conn, "claude").unwrap();
        assert_eq!(list.len(), 2);
        // sort_index=1 的 p2 应排在前
        assert_eq!(list[0].id, "p2");
        assert!(!list[0].settings_json_valid, "非法 JSON 应被标记");
        assert!(list[1].id == "p1" && list[1].settings_json_valid);
    }

    #[test]
    fn rejects_unsupported_app_type() {
        let conn = setup_min_db();
        assert!(list_provider_summaries(&conn, "gemini").is_err());
        assert!(list_provider_summaries(&conn, "claude; DROP TABLE providers").is_err());
    }

    #[test]
    fn query_only_blocks_writes() {
        let conn = setup_min_db();
        conn.pragma_update(None, "query_only", "ON").unwrap();
        let err = conn.execute("INSERT INTO providers (id, app_type, name, settings_config) VALUES ('x','pi','x','{}')", []);
        assert!(err.is_err(), "query_only 连接禁止写入");
    }
}
