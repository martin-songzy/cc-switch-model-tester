# 最终验收清单（DevelopmentPlan.md 第 21 节，22 条）

核对日期：2026-09-11 · 版本：M6（便携版 dist\cc-switch-model-tester.exe）

| # | 验收标准 | 状态 | 证据 |
|---|---|---|---|
| 1 | 读取 cc-switch 3.20.1 schema 18 | ✅ | 支持 schema 16~18（`ccswitch/db.rs`），实机 39 供应商读取正常 |
| 2 | Claude/Codex/Pi 三标签显示供应商与模型 | ✅ | 实机验证；`load_provider_catalog` 三类均出数 |
| 3 | 每个解析分支有脱敏 fixture | ✅ | `tests/fixtures/` 12 个脱敏 fixture（`tools/gen-fixtures.mjs` 生成，不含真实 Key） |
| 4 | 四协议均支持非流式+流式 | ✅ | protocol_tests 24 项覆盖 Anthropic / OpenAI Chat / OpenAI Responses / Gemini Native |
| 5 | Pi 三协议复用适配器；Bedrock 跳过 | ✅ | `parser/pi.rs` 映射三类；Bedrock → unsupported_protocol，不发请求 |
| 6 | 每模型严格执行测试次数 | ✅ | `dedup::expand_provider` 按 attempts_per_model 展开；测试锁定 |
| 7 | 全局/单供应商并发不超限 | ✅ | 两级 Semaphore（`scheduler.rs`） |
| 8 | 200+error / 空响应 / 超上限 / 负面词判失败 | ✅ | `judge.rs` 分类链 + judge 单测 |
| 9 | 失败不自动重试 | ✅ | 无重试逻辑；失败即记录 |
| 10 | 代理失败不回退直连 | ✅ | 请求失败直接归类网络错误 |
| 11 | 内置提示词不含 hello/hi/test/ping，可增删改 | ✅ | 8 条短提示词 + M5 提示词管理（内置可改不可删） |
| 12 | 历史只含摘要，不含完整响应和明文 Key | ✅ | `test_attempts` 仅存 response_summary；Key 只有 credential_hint（末 4 位） |
| 13 | 活动测试可查看完整响应，退出后释放 | ✅ | 实时明细驻内存（AttemptFinished），不落库；应用退出即释放 |
| 14 | 取消停止排队任务与进行中请求 | ✅ | CancellationToken + tokio::select! 中断 HTTP |
| 15 | Windows Release 可直接运行 | ✅ | 便携版 exe（10.7MB）实机运行验证；MSI 按用户要求取消 |
| 16 | 测试/日志/错误提示无明文 Key | ✅ | `tools/scan-secrets.mjs` 扫描源码/exe/日志/数据库 = 0 命中；redact 覆盖所有日志点 |
| 17 | 不写 cc-switch 数据库及 live 配置 | ✅ | SQLite 只读方式打开（SQLITE_OPEN_READ_ONLY） |
| 18 | 可区分 8+ 种结果状态 | ✅ | 通过/网络错误/鉴权失败/HTTP错误/负面规则/错误对象/空响应/解析失败/已取消 |
| 19 | 精确等价目标去重 + 预览提示减少数 | ✅ | 10 字段指纹去重；预览展示 original→deduplicated 计数 |
| 20 | 不因名称/ID/域名相同误合并 | ✅ | 指纹含凭据哈希与完整端点；dedup_tests 9 项锁定 |
| 21 | 代表结果保留被合并来源 | ✅ | merged_sources 随记录/事件返回，UI 可见 |
| 22 | 预览不发请求；配置变化须重新预览 | ✅ | 预览纯内存计算；raw_config_hash 快照校验 |

**结论：22/22 通过。**（自动化测试 114 项全绿；svelte-check 0 错误 0 警告；Rust 警告清零；Key 泄露扫描通过）
