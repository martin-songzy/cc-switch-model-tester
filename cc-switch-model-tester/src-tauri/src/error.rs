//! 统一错误框架。
//!
//! 所有对外（前端 / 日志 / 历史）错误必须携带机器分类 + 用户可读说明，
//! 参见 DevelopmentPlan.md 第 16 节。

use serde::ser::Serializer;
use serde::Serialize;

/// 机器可读错误分类（snake_case 发送给前端）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppErrorKind {
    /// 通用 IO 错误
    Io,
    /// 工具自身数据库错误
    Store,
    /// cc-switch 数据库不存在
    CcSwitchDbNotFound,
    /// 文件不是合法的 SQLite 数据库
    CcSwitchDbNotSqlite,
    /// cc-switch 数据库 schema 版本不受支持
    CcSwitchDbUnsupportedVersion,
    /// cc-switch 数据库打开失败（被锁定 / 无权限等）
    CcSwitchDbOpenFailed,
    /// 供应商配置无效
    ProviderConfigInvalid,
    /// 用户输入校验失败
    Validation,
    /// 代理错误（后续里程碑使用）
    Proxy,
    /// 网络错误（后续里程碑使用）
    Network,
    /// 内部错误
    Internal,
}

/// 统一应用错误：分类 + 用户可读说明。
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct AppError {
    pub kind: AppErrorKind,
    pub message: String,
}

impl AppError {
    pub fn new(kind: AppErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::new(AppErrorKind::Io, e.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::new(AppErrorKind::Store, e.to_string())
    }
}

/// 序列化为 `{ kind, message }`，供前端展示。
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Repr<'a> {
            kind: AppErrorKind,
            message: &'a str,
        }
        Repr {
            kind: self.kind,
            message: &self.message,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_kind_and_message() {
        let e = AppError::new(
            AppErrorKind::CcSwitchDbNotFound,
            "数据库文件不存在: C:\\x\\y.db",
        );
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("cc_switch_db_not_found"));
        assert!(json.contains("数据库文件不存在"));
    }
}
