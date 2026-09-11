//! 测试提示词选择（DevelopmentPlan.md 9.1 / 9.2）。
//!
//! - 每次尝试随机选择一个已启用提示词
//! - 同一模型连续两次尝试不重复（启用提示词 < 2 时允许重复）
//! - 随机数由 Rust 生成

use rand::{Rng, RngExt};

use crate::error::{AppError, AppErrorKind};

#[derive(Debug, Clone)]
pub struct PromptItem {
    pub id: String,
    pub content: String,
}

/// 从 SQLite 读取已启用提示词。
pub fn load_enabled_prompts(conn: &rusqlite::Connection) -> Result<Vec<PromptItem>, AppError> {
    let mut stmt =
        conn.prepare("SELECT id, content FROM prompt_templates WHERE enabled = 1 ORDER BY id")?;
    let rows = stmt.query_map([], |row| {
        Ok(PromptItem {
            id: row.get(0)?,
            content: row.get(1)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 随机选择提示词，避免与 `last_prompt_id` 相同（除非只有一个可用）。
/// 返回 None 表示没有启用提示词（禁止开始测试）。
pub fn pick_random(
    prompts: &[PromptItem],
    last_prompt_id: Option<&str>,
    rng: &mut impl Rng,
) -> Option<PromptItem> {
    if prompts.is_empty() {
        return None;
    }
    if prompts.len() == 1 {
        return Some(prompts[0].clone());
    }
    loop {
        let idx = rng.random_range(0..prompts.len());
        let candidate = &prompts[idx];
        if Some(candidate.id.as_str()) != last_prompt_id {
            return Some(candidate.clone());
        }
        // 抽到重复的再抽一次（有限次后必然不同）
    }
}

/// 校验：全部提示词禁用时禁止开始测试（文档 9.3）。
pub fn ensure_any_enabled(conn: &rusqlite::Connection) -> Result<(), AppError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM prompt_templates WHERE enabled = 1",
        [],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(AppError::new(
            AppErrorKind::ProviderConfigInvalid,
            "没有已启用的测试提示词，请先在提示词管理中启用至少一条",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn items(ids: &[&str]) -> Vec<PromptItem> {
        ids.iter()
            .map(|s| PromptItem {
                id: s.to_string(),
                content: format!("content-{s}"),
            })
            .collect()
    }

    #[test]
    fn picks_from_enabled() {
        let mut rng = StdRng::seed_from_u64(42);
        let ps = items(&["a", "b", "c"]);
        for _ in 0..20 {
            let p = pick_random(&ps, None, &mut rng).unwrap();
            assert!(ps.iter().any(|x| x.id == p.id));
        }
    }

    #[test]
    fn avoids_repeat_when_possible() {
        let mut rng = StdRng::seed_from_u64(42);
        let ps = items(&["a", "b"]);
        for last in ["a", "b"] {
            for _ in 0..20 {
                let p = pick_random(&ps, Some(last), &mut rng).unwrap();
                assert_ne!(p.id, last, "两个可用时不应与上次重复");
            }
        }
    }

    #[test]
    fn single_prompt_may_repeat() {
        let mut rng = StdRng::seed_from_u64(42);
        let ps = items(&["only"]);
        let p = pick_random(&ps, Some("only"), &mut rng).unwrap();
        assert_eq!(p.id, "only");
    }

    #[test]
    fn empty_returns_none() {
        let mut rng = StdRng::seed_from_u64(42);
        assert!(pick_random(&[], None, &mut rng).is_none());
    }
}
