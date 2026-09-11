# cc-switch Model Tester

批量测试 [cc-switch](https://github.com/farion1231/cc-switch) 管理的 LLM API 供应商/模型是否真正可用，覆盖 **Claude Code / Codex / Pi agent** 三类应用。

不是简单 ping 一下接口，而是按各应用的真实协议发送完整对话请求，并根据响应内容判定真实可用性。

## 功能亮点

- **统一目录**：直接读取 cc-switch 数据库（只读），三标签页管理所有供应商与模型
- **四协议适配**：Anthropic / OpenAI Chat / OpenAI Responses / Gemini Native，均支持流式与非流式
- **真实判定**：HTTP 200 不等于可用——空响应、错误对象、负面关键词、解析失败都会被识别为失败
- **智能去重**：完全相同的测试目标自动合并，发送前预览去重明细
- **两级并发控制**：全局并发 + 单供应商并发，失败不自动重试
- **测试历史**：自动保留最近 15 分钟，按应用分类，API Key 只显示末 4 位
- **可定制**：测试提示词与负面规则均可增删改，内置 8 条短提示词
- **本地隐私**：数据不出本机，日志/历史/错误提示中的密钥全程脱敏

## 下载使用（Windows 10/11）

便携版，无需安装：

1. 打开 [Actions 构建页](https://github.com/martin-songzy/cc-switch-model-tester/actions)
2. 点最新一次绿色 ✅ 构建
3. 页面底部 **Artifacts** 下载 `cc-switch-model-tester-windows`，解压双击运行

前置要求：本机安装并配置过 cc-switch 3.20.x（数据库位于 `~/.cc-switch/cc-switch.db`）。

详细使用说明（界面、参数、结果类别、常见问题、安全说明）见 **[使用手册](cc-switch-model-tester/README.md)**。

## 仓库结构

```
├── DevelopmentPlan.md            开发计划（里程碑 / 验收标准）
├── Requirement.md                需求文档
├── cc-switch-model-tester/       应用源码
│   ├── README.md                 使用手册
│   ├── src/                      前端（Svelte 5）
│   ├── src-tauri/                后端（Rust + Tauri 2）
│   ├── src-tauri/tests/          脱敏 fixture + 114 项自动化测试
│   └── docs/acceptance.md        验收清单（22 条全过）
└── .github/workflows/build.yml   云端编译（push 自动触发）
```

## 开发

```powershell
cd cc-switch-model-tester

npm run check                          # 前端类型检查
npm run tauri build -- --no-bundle     # 本地构建便携版
cd src-tauri; cargo test               # 后端全量测试
node tools/scan-secrets.mjs            # Key 泄露扫描（发布前必跑）
```

技术栈：Tauri 2 + Rust + Svelte 5。推送即自动云端编译（Windows Runner，含类型检查与 Rust 缓存，约 8 分钟）。

## 安全设计

- 对 cc-switch 数据库**只读**，绝不写入其配置
- API Key 全程留本机内存；界面与历史只显示末尾 4 位
- 日志、错误提示中的密钥与 URL 参数自动脱敏；`tools/scan-secrets.mjs` 可对产物/日志/数据库做泄露扫描
- 仓库内测试样本（fixtures）全部使用脱敏假数据
