# cc-switch 模型可用性测试工具

## 1. 文档目的

本文档是第一版开发规格，不是概念方案。第三方开发模型应以本文档作为实现依据；当本文档与代码库中的 `Requirement.md` 存在差异时，以本文档中已冻结的范围、约束和验收标准为准。

项目当前仅包含需求文件，开发模型需要从零创建 Windows GUI 项目。

## 2. 产品定义

产品名称暂定：`cc-switch Model Tester`。

产品目标：

- 只读加载 cc-switch 3.20.1 保存的供应商配置。
- 让用户选择 Claude Code、Codex、Pi agent 下的供应商和模型。
- 直接向供应商上游发送低成本、正常的编程类提示词。
- 用真实模型响应判断模型是否可用以及多次请求是否稳定。
- 不修改 cc-switch 数据库、配置文件或当前供应商状态。
- 只做 Windows GUI，不提供 CLI、服务端和远程控制接口。

### 2.1 已冻结范围

- 只支持 cc-switch `v3.20.1` 的当前 SQLite 数据库格式。
- 只读取 cc-switch 数据库，不兼容旧版 JSON 供应商文件。
- 直连供应商上游，不经过 cc-switch 本地代理。
- 不支持 Codex OAuth、Claude OAuth、GitHub Copilot、xAI OAuth 等托管账号。
- 每个 `供应商 + 模型` 是独立测试对象。
- 同一测试批次内发现完全相同的有效测试目标时，开始测试前自动去重并提示；只按请求目标精确去重，不按供应商名称或 ID 粗略合并。
- 默认每个模型测试 3 次，次数可配置。
- 全局并发默认 10。
- 单供应商并发默认 1。
- 支持快速非流式和兼容性流式两种测试模式。
- 默认代理为 `socks5://127.0.0.1:1080`。
- 默认所有供应商禁止简单 `hello/hi/test/ping` 探测，使用内置编程提示词随机测试。
- 提示词支持新增、编辑、删除、启用和禁用。
- 测试历史保存摘要，不保存完整模型响应正文。
- 测试运行期间允许用户主动打开详情查看完整响应；程序退出后完整响应不保留。
- 不因代理不可用而静默回退直连。
- 失败不自动重试；每次尝试都必须真实计入稳定性统计。
- `provider_endpoints` 默认只作为候选端点显示；默认只测试主端点，用户开启“测试全部候选端点”后才逐一测试。开启后，每个端点都形成独立测试对象和独立统计，不把多个端点混成一个成功率。
- Bedrock 格式可以被识别和展示，但第一版不实现 AWS SigV4 或 Bedrock 凭据读取；缺少受支持认证时标记为“暂不支持”，不发送请求。

### 2.2 明确非目标

- 不实现 cc-switch 的供应商切换功能。
- 不启用或修改 cc-switch 本地代理接管。
- 不启动 Claude Code、Codex 或 Pi 进程。
- 不读取官方 OAuth token 或共享认证状态。
- 不读取系统环境变量作为 API Key 的隐式兜底。
- 不上传配置、提示词、响应或测试结果。
- 不承诺规避供应商风控或封禁；正常提示词只用于避免明显的 hello 探测。
- 不做压力测试。测试并发只是批量执行控制，不代表供应商容量压测。

## 3. 外部参考资料

开发时应优先参考以下固定版本资料，不要只参考 `main` 分支。

### 3.1 cc-switch 3.20.1

- 项目主页：<https://github.com/farion1231/cc-switch>
- 3.20.1 发布说明：<https://github.com/farion1231/cc-switch/blob/v3.20.1/docs/release-notes/v3.20.1-zh.md>
- 配置文件说明：<https://github.com/farion1231/cc-switch/blob/v3.20.1/docs/user-manual/zh/5-faq/5.1-config-files.md>
- Provider 数据结构：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src-tauri/src/provider.rs>
- SQLite 建表和 schema 迁移：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src-tauri/src/database/schema.rs>
- SQLite 初始化和默认路径：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src-tauri/src/database/mod.rs>
- Pi 原生配置读取：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src-tauri/src/pi_config/mod.rs>
- Live 配置读写逻辑：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src-tauri/src/services/provider/live.rs>
- 应用和协议类型：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src/types.ts>
- Pi 协议和供应商预设：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src/config/piProviderPresets.ts>
- Claude 供应商预设：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src/config/claudeProviderPresets.ts>
- Codex 供应商预设：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src/config/codexProviderPresets.ts>
- Claude 表单和上游格式：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src/components/providers/forms/ClaudeFormFields.tsx>
- Codex 表单和上游格式：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src/components/providers/forms/CodexFormFields.tsx>
- Pi 表单和原生字段：<https://github.com/farion1231/cc-switch/blob/v3.20.1/src/components/providers/forms/PiProviderForm.tsx>
- Codex 3.20.1 config-only 认证说明：<https://github.com/farion1231/cc-switch/blob/v3.20.1/docs/guides/codex-official-auth-preservation-guide-zh.md>

### 3.2 上游协议

- Anthropic Messages API：<https://docs.anthropic.com/en/api/messages>
- Anthropic Streaming：<https://docs.anthropic.com/en/api/messages-streaming>
- OpenAI Chat Completions：<https://platform.openai.com/docs/api-reference/chat>
- OpenAI Responses：<https://platform.openai.com/docs/api-reference/responses>
- Gemini generateContent：<https://ai.google.dev/api/generate-content>
- Gemini streaming：<https://ai.google.dev/api/generate-content#method:-streamgeneratecontent>
- Amazon Bedrock ConverseStream：<https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_ConverseStream.html>
- Bedrock API Key：<https://docs.aws.amazon.com/bedrock/latest/userguide/api-keys-use.html>

Bedrock 文档仅用于协议边界参考。第一版不实现 AWS SigV4、AWS Access Key/Secret Key、Region 凭据发现和 Bedrock API Key 读取。

## 4. 总体架构

```text
Tauri 2 Windows Application
├── Rust Core
│   ├── cc-switch SQLite read-only source
│   ├── Claude/Codex/Pi config normalizers
│   ├── protocol request adapters
│   ├── HTTP client and proxy
│   ├── scheduler and concurrency limits
│   ├── response parser and verdict engine
│   └── local SQLite history
└── Svelte + TypeScript UI
    ├── Claude Code tab
    ├── Codex tab
    ├── Pi agent tab
    ├── test controls
    ├── prompt editor
    └── result table and active-run details
```

核心原则：

1. 所有 API Key 读取、请求构造、HTTP 通信、响应解析和数据库操作放在 Rust。
2. 前端不接收明文 API Key，不保存供应商原始 `settings_config`。
3. cc-switch 数据库连接启用只读模式，应用永远不执行 INSERT/UPDATE/DELETE。
4. 一个测试批次启动时创建不可变配置快照；测试过程中不重新读取供应商配置。
5. 协议适配器与调度器解耦，新增协议不应修改并发调度代码。

推荐技术栈：

```text
Rust 1.80+，具体版本以 Tauri 2 兼容版本为准
Tauri 2
Tokio
reqwest + socks
rusqlite
serde / serde_json
toml_edit
url
regex
chrono
tracing
Svelte + TypeScript
```

## 5. cc-switch 数据读取

### 5.1 数据库路径

默认路径：

```text
%USERPROFILE%\.cc-switch\cc-switch.db
```

必须支持：

- 启动时自动发现默认路径。
- 用户通过文件选择器手动选择 `cc-switch.db` 或 cc-switch 数据目录。
- Codex 外部 `model_catalog_json` 的解析基准目录单独设置：默认 `%USERPROFILE%\.codex`，用户可在“数据源设置”中指定 Codex 配置目录；这个目录只用于读取本地模型目录，不用于读取 `auth.json`。
- 路径不存在、无权限、不是 SQLite、数据库版本过新时给出明确错误。
- 仅支持 schema `user_version = 18`。如果版本高于 18，停止读取并提示升级工具；不做猜测兼容。
- 如果版本低于 18，提示用户先升级 cc-switch 3.20.1，不执行迁移。

### 5.2 只读连接

使用 SQLite URI 或等价方式实现只读连接，并设置：

```sql
PRAGMA query_only = ON;
PRAGMA foreign_keys = ON;
```

禁止调用 cc-switch 的数据库迁移逻辑。工具只读现有表。

首次加载的主要查询：

```sql
SELECT
  id,
  app_type,
  name,
  settings_config,
  website_url,
  category,
  created_at,
  sort_index,
  notes,
  icon,
  icon_color,
  meta,
  is_current
FROM providers
WHERE app_type IN ('claude', 'codex', 'pi')
ORDER BY app_type, COALESCE(sort_index, 9223372036854775807), name;
```

端点候选：

```sql
SELECT provider_id, app_type, url, added_at
FROM provider_endpoints
WHERE app_type IN ('claude', 'codex', 'pi')
ORDER BY app_type, provider_id, added_at;
```

### 5.3 Provider 过滤

读取后进行以下过滤：

- `app_type` 只接受 `claude`、`codex`、`pi`。
- `settings_config` 不是合法 JSON 的供应商显示为“配置错误”，不能进入可执行队列。
- 运行时配置源只有 cc-switch `providers` 表和同库 `provider_endpoints` 表；不读取 Claude/Codex/Pi 的 live 文件作为回退，不直接读取 Pi 的 `~/.pi/agent/models.json`。
- Pi 的 `models.json` 是 cc-switch 同步原生配置时的外部设计参考；本工具的实际输入仍然是数据库中已保存的该 Pi provider `settings_config`。
- 检测 `meta.providerType` 以及已知托管 URL/标识。
- `codex_oauth`、`github_copilot`、`xai_oauth` 等托管账号供应商显示为“已跳过：托管认证不在第一版范围”，不能发送请求。
- 不因 `category` 是官方或第三方而自动判断可用性；是否可测取决于协议和凭据是否能从数据库配置中解析。

### 5.4 配置源和字段安全

- 运行时 provider 与模型的唯一主来源是 cc-switch SQLite 中的 `providers.settings_config`。
- 同库 `provider_endpoints` 仅提供候选端点；它不覆盖主配置中的 Base URL，只在用户开启“测试全部候选端点”时展开测试对象。
- Codex 的本地外部 `model_catalog_json` 文件是用户选择的补充来源，仅补充模型目录，不覆盖 SQLite 中的 provider、协议、认证或主端点。
- Claude/Codex/Pi 的 live 配置文件不参与运行时回退；Pi native 文件只作为字段形状参考。
- 每个动态 JSON/TOML 文件都要设置大小上限、解析错误分类和未知字段保留策略。
- 未知字段不参与协议猜测，也不应导致已知字段丢失；将其保存在内部 passthrough 或原始哈希中，绝不写回 cc-switch。

### 5.5 主端点与候选端点

- 主端点来自该 provider `settings_config` 解析出的 Base URL。
- 候选端点来自同库 `provider_endpoints`，按 URL 去重。
- 默认测试主端点一次；开启全部候选端点时，主端点加候选端点逐一展开。
- 同一端点的多个模型共享 `(app_type, provider_id)` 的供应商并发限制。
- 结果主键语义必须包含 `run_id + app_type + provider_id + endpoint_url + model_id + protocol + mode + attempt_no`，避免多个端点、应用或协议覆盖结果。

### 5.6 配置快照

启动测试时一次性读取并规范化所有选中 provider。快照包含脱敏后的展示信息、配置哈希和请求所需的 Rust 内存凭据；测试运行中不重新读取数据库，也不读取 live 配置。

### 5.7 统一领域模型

```rust
pub enum AppType {
    Claude,
    Codex,
    Pi,
}

pub enum ApiProtocol {
    AnthropicMessages,
    OpenAiChat,
    OpenAiResponses,
    GeminiNative,
    PiOpenAiCompletions,
    PiOpenAiResponses,
    PiAnthropicMessages,
    PiGoogleGenerativeAi,
    PiBedrockConverseStream,
}

pub enum TestMode {
    NonStreaming,
    Streaming,
}

pub struct ProviderSnapshot {
    pub app: AppType,
    pub provider_id: String,
    pub provider_name: String,
    pub endpoint: String,
    pub candidate_endpoints: Vec<String>,
    pub protocol: ApiProtocol,
    pub models: Vec<ModelSnapshot>,
    pub credential: CredentialSnapshot,
    pub headers: Vec<(String, String)>,
    pub custom_user_agent: Option<String>,
    pub full_url: bool,
    pub raw_config_hash: String,
}

pub struct ModelSnapshot {
    pub model_id: String,
    pub display_name: Option<String>,
    pub roles: Vec<String>,
    pub base_url_override: Option<String>,
    pub compat: serde_json::Value,
}

pub struct TestTarget {
    pub app: AppType,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_display_name: Option<String>,
    pub endpoint_url: String,
    pub protocol: ApiProtocol,
    pub credential: CredentialSnapshot,
    pub headers: Vec<(String, String)>,
    pub custom_user_agent: Option<String>,
    pub full_url: bool,
    pub compat: serde_json::Value,
    pub dedup_key_hash: String,
    pub source_refs: Vec<TargetSourceRef>,
}

pub struct TargetSourceRef {
    pub app: AppType,
    pub provider_id: String,
    pub provider_name: String,
    pub model_key: String,
}

pub struct DedupSummary {
    pub original_target_count: usize,
    pub deduplicated_target_count: usize,
    pub original_attempt_count: usize,
    pub deduplicated_attempt_count: usize,
    pub duplicate_group_count: usize,
    pub removed_attempt_count: usize,
}

pub struct DedupPreview {
    pub preview_id: String,
    pub config_snapshot_hash: String,
    pub summary: DedupSummary,
    pub groups: Vec<DedupGroupPreview>,
    pub expires_at: i64,
}

pub struct DedupGroupPreview {
    pub dedup_key_hash: String,
    pub representative: TargetDisplay,
    pub merged_sources: Vec<TargetSourceRef>,
    pub removed_target_count: usize,
    pub removed_attempt_count: usize,
}

pub struct TargetDisplay {
    pub app: AppType,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub endpoint_display: String,
    pub protocol: ApiProtocol,
    pub mode: TestMode,
}

pub struct DedupPreview {
    pub preview_id: String,
    pub config_snapshot_hash: String,
    pub summary: DedupSummary,
    pub groups: Vec<DedupGroupPreview>,
    pub expires_at: i64,
}

pub struct DedupGroupPreview {
    pub dedup_key_hash: String,
    pub representative: TargetDisplay,
    pub merged_sources: Vec<TargetSourceRef>,
    pub removed_target_count: usize,
    pub removed_attempt_count: usize,
}

pub struct TargetDisplay {
    pub app: AppType,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub endpoint_display: String,
    pub protocol: ApiProtocol,
    pub mode: TestMode,
}

pub struct TestRunInput {
    pub app: AppType,
    pub provider_ids: Vec<String>,
    pub model_keys: Vec<String>,
    pub attempts_per_model: u32,
    pub mode: TestMode,
    pub global_concurrency: u32,
    pub provider_concurrency: u32,
    pub test_all_candidate_endpoints: bool,
    pub apply_body_overrides: bool,
}
```

`CredentialSnapshot` 只在 Rust 内存中存在；任何发送到前端或写入历史库的对象都必须使用 `CredentialKind` 和脱敏信息，不允许包含明文。

### 5.8 重复目标识别

重复识别发生在用户点击“开始测试”之后、任何真实请求发送之前。一次测试批次内的所有已选目标先展开，再统一去重。当前 UI 的三个标签页默认各自创建独立批次，因此默认只在当前标签页内去重；只有实现共享测试批次后，才允许跨 Claude Code、Codex、Pi agent 去重。

精确去重键由以下规范化字段组成，并在 Rust 内存中计算 SHA-256：

```text
canonical_endpoint_url
canonical_protocol
actual_model_id
test_mode
credential_kind + credential_fingerprint
canonical_effective_headers
custom_user_agent
full_url
canonical_compat_request_fields
body_override_hash（仅在启用 Body override 时）
```

规范化规则：

- endpoint 去除首尾空白，scheme/host 小写化，去掉多余末尾 `/`，保留实际路径和有意义的 query 参数；默认端口归一化。
- 模型 ID 使用去除 cc-switch 展示标记后的真实 ID。
- 协议使用统一枚举值；例如 Pi `openai-completions` 和 Claude/Codex 的 `openai_chat` 都归一为 `OpenAiChat`，但必须保留原始来源协议用于显示。
- Header 名称大小写不敏感、按名称和值排序；认证值只参与内部指纹，不发送到前端或历史库。
- 不能仅因为 provider 名称、provider ID、Base URL 域名或模型名称相同就去重。
- 同一端点但 API Key 不同、协议不同、模型不同、测试模式不同、有效 Header 不同或兼容请求字段不同，都不是重复目标。
- 主端点和候选端点只有在规范化 URL 完全相同时才去重；不同端点分别测试。

去重结果：

```rust
pub struct DedupGroup {
    pub dedup_key_hash: String,
    pub representative: TestTarget,
    pub merged_sources: Vec<TargetSourceRef>,
    pub removed_target_count: usize,
    pub removed_attempt_count: usize,
}
```

选择代表项时使用稳定顺序：标签页顺序 `Claude Code -> Codex -> Pi agent`，然后按 cc-switch `sort_index`、供应商名称、provider ID 和模型 ID 排序。代表项只发送一次请求；被合并项不再发送请求，但其来源信息写入当前运行摘要。

如果一组目标原本执行 `N` 次，去重后该组仍只执行 `N` 次，而不是 `N × 来源数` 次。提示中的“减少请求数”按去重前后 attempt 数量计算。

### 5.9 重复提示时机

- 没有重复目标时不弹窗。
- 发现重复目标时，在开始发送请求前显示确认对话框。
- 对话框显示：重复组数量、被合并来源数量、去重前 attempt 数、去重后 attempt 数、预计减少请求数。
- 对话框提供“查看重复项”，逐组显示代表目标和被合并的应用/供应商/模型。
- 用户选择取消时，整个运行保持 `cancelled_before_start`，不发送任何请求、不写入成功率统计。
- 用户确认后才创建正式测试批次并发送请求。

## 6. 三类配置解析

### 6.1 Claude Code

从 `settings_config.env` 读取：

```text
ANTHROPIC_BASE_URL
ANTHROPIC_API_KEY
ANTHROPIC_AUTH_TOKEN
ANTHROPIC_MODEL
ANTHROPIC_DEFAULT_HAIKU_MODEL
ANTHROPIC_DEFAULT_SONNET_MODEL
ANTHROPIC_DEFAULT_OPUS_MODEL
ANTHROPIC_DEFAULT_FABLE_MODEL
CLAUDE_CODE_SUBAGENT_MODEL
```

规则：

- `meta.apiFormat` 是首选协议字段；其优先级遵循 cc-switch `get_claude_api_format`：`meta.apiFormat` > `settings_config.api_format` > legacy `openrouter_compat_mode` > 默认 `anthropic`。
- 支持的 Claude 协议值为 `anthropic`、`openai_chat`、`openai_responses`、`gemini_native`；未知值按配置错误处理，不静默当作 Anthropic。
- 模型字段 `ANTHROPIC_MODEL`、`ANTHROPIC_DEFAULT_HAIKU_MODEL`、`ANTHROPIC_DEFAULT_SONNET_MODEL`、`ANTHROPIC_DEFAULT_OPUS_MODEL`、`ANTHROPIC_DEFAULT_FABLE_MODEL` 和 `CLAUDE_CODE_SUBAGENT_MODEL` 全部加入模型集合后去重。
- `*_MODEL_NAME` 只作为显示名称，不作为真实模型 ID；`[1M]` 等 cc-switch 模型标记必须从 ID 中解析为能力元数据，并按上游实际 ID 发送（不要把标记原样发送给供应商）。
- Claude 凭据按 cc-switch adapter 的配置优先级读取：`ANTHROPIC_AUTH_TOKEN` > `ANTHROPIC_API_KEY` > `OPENROUTER_API_KEY` > `OPENAI_API_KEY` > `GEMINI_API_KEY` > 顶层 `apiKey`/`api_key`。
- 认证策略根据实际命中的字段选择：`ANTHROPIC_AUTH_TOKEN` 使用 `Authorization: Bearer`；`ANTHROPIC_API_KEY` 使用 `x-api-key`；OpenAI/OpenRouter/Gemini 兼容字段按对应协议适配器规则发送。
- 只使用配置内非空认证值，不读取系统环境变量。
- `meta.isFullUrl` 用于阻止自动追加 API 路径；`meta.customUserAgent` 和 `meta.localProxyRequestOverrides.headers` 作为配置快照的一部分处理。

Claude 协议值：

```text
anthropic          -> Anthropic Messages
openai_chat        -> OpenAI Chat Completions
openai_responses   -> OpenAI Responses
gemini_native      -> Gemini Native generateContent
```

### 6.2 Codex

`settings_config` 中的 `config` 是 TOML 文本，必须使用 TOML 解析器解析，不能用正则替代完整解析。

重点字段：

```toml
model_provider = "custom"
model = "model-id"
model_catalog_json = "..."

[model_providers.custom]
name = "Provider"
base_url = "https://example.com"
wire_api = "responses"
experimental_bearer_token = "sk-..."
requires_openai_auth = false
http_headers = { "X-Provider-Mode" = "coding" }
# env_http_headers 只记录为待解析警告；本工具不读取系统环境变量
```

规则：

- API Key 首先按 cc-switch 3.20.1 的 `extract_codex_api_key` 语义读取数据库 `settings_config.auth.OPENAI_API_KEY`；如果该字段为空，再从 `config` TOML 中当前 provider 的 `experimental_bearer_token` 读取。
- `settings_config.auth` 中的 `tokens`、ChatGPT 登录信息和任何 OAuth 字段都不作为本工具凭据。
- 不读取 live `~/.codex/auth.json`，不读取系统环境变量，也不从其他 provider 表借用密钥。
- `base_url` 只从当前 `model_provider` 对应表读取；若当前 provider 表没有，才按 cc-switch helper 规则回退顶层 `base_url`，绝不从非活动 provider 表猜测。
- Codex 的上游协议由 cc-switch `meta.apiFormat` 读取，支持：
  - `openai_responses`
  - `openai_chat`
  - `anthropic`
- `wire_api = responses` 是 Codex 客户端侧配置，不应被误认为上游一定是 Responses；如果 3.20.1 配置声明了非 `responses` 的客户端 wire 值，应显示配置警告，但本工具仍按 `meta.apiFormat` 选择上游适配器。
- `model` 是基础测试模型。
- 如果数据库 `settings_config.modelCatalog.models` 存在，优先解析其模型目录并加入模型集合；这是 cc-switch 的 DB SSOT。
- 如果数据库配置没有 `modelCatalog.models`，但 `config` TOML 中存在 `model_catalog_json`，允许读取该字段指向的本地 JSON 模型目录作为补充来源；该读取仅限本地文件，不发网络请求，不读取 live `config.toml` 来覆盖数据库快照。
- `model_catalog_json` 为相对路径时，以用户设置的 Codex 配置目录为基准；默认目录为 `%USERPROFILE%\.codex`。绝对路径仅在文件位于该配置目录内时读取；拒绝目录穿越和符号链接逃逸。
- 外部模型目录读取设置 32 MiB 文件上限，必须是合法 JSON 且包含 `models` 数组；文件不存在、越界或解析失败时使用顶层 `model`，同时显示配置警告。
- 外部模型目录条目按 cc-switch 3.20.1 生成目录的字段读取：`slug` 是 Codex 目录中的模型标识，映射为测试用 `model_id`；`display_name` 映射为显示名；`context_window`、`base_instructions`、`input_modalities`、`supports_reasoning_summaries` 等能力字段只作为请求兼容元数据，不改变模型 ID。
- 数据库 `modelCatalog.models` 的简化条目使用 `model` 和 `displayName`；外部完整目录使用 `slug` 和 `display_name`。两种格式都必须支持并去重。
- Codex 的 provider 表中 `http_headers` 的字面量 Header 必须纳入快照；`env_http_headers` 如果引用进程环境变量，第一版不解析，显示“依赖环境变量，无法按 cc-switch 数据库独立重现”。
- `http_headers` 和 Pi 的 `headers` 是供应商配置的一部分；Claude 的同类附加 Header 主要来自 `meta.localProxyRequestOverrides.headers`。
- 如果 `requires_openai_auth = true` 且按上述规则没有供应商自身凭据，标记为缺少凭据，不发送请求。

### 6.3 Pi agent

运行时不直接读取 Pi 的 native 文件；只解析 cc-switch `providers` 表中 `app_type = 'pi'` 的 `settings_config`。以下 `models.json` 是 cc-switch 3.20.1 原生配置形状，用于编写 fixture 和校验字段映射：

默认原生参考文件（仅供核对，不是本工具运行时数据源）：

```text
%USERPROFILE%\.pi\agent\models.json
```

对应原生结构：

```json
{
  "providers": {
    "provider-key": {
      "name": "Provider",
      "baseUrl": "https://example.com/v1",
      "api": "openai-completions",
      "apiKey": "sk-xxx",
      "headers": {
        "X-Title": "example"
      },
      "models": [
        {
          "id": "model-id",
          "name": "Model Name",
          "baseUrl": "https://model.example.com",
          "reasoning": true,
          "input": ["text"],
          "contextWindow": 128000,
          "maxTokens": 4096,
          "thinkingLevelMap": {},
          "compat": {},
          "futureProviderField": "preserve"
        }
      ]
    }
  }
}
```

规则：

- 供应商级 `baseUrl` 是默认端点。
- 模型级 `baseUrl` 覆盖供应商级端点。
- 供应商级 `apiKey` 是默认凭据。
- 供应商级 `headers` 作为请求自定义 Header。
- 模型级 `compat` 与供应商级 `compat` 做对象合并，模型级字段优先。
- `thinkingLevelMap` 是 Pi 的模型级思考级别映射，应原样保留到快照，用于兼容性判断，但不把它误认为通用 API 参数。
- 未被 cc-switch 表单控制的 provider/model 字段必须原样保留到快照的 passthrough 区域；测试器不应因为未知字段而丢弃配置语义。
- `models[]` 的 `id` 是实际请求模型 ID，`name` 只做显示。
- 同一供应商多个模型分别生成测试目标。
- 没有 `models[]` 的供应商显示配置错误，不能凭空测试未知模型。

Pi 当前协议值：

```text
openai-completions       -> OpenAI Chat Completions
openai-responses         -> OpenAI Responses
anthropic-messages       -> Anthropic Messages
google-generative-ai     -> Gemini Native
bedrock-converse-stream  -> Bedrock ConverseStream，第一版仅识别，不执行
```

## 7. 协议适配器

统一接口建议：

```rust
#[async_trait]
pub trait ProtocolAdapter: Send + Sync {
    fn protocol(&self) -> ApiProtocol;

    fn build_request(
        &self,
        target: &TestTarget,
        prompt: &str,
        mode: TestMode,
    ) -> Result<PreparedRequest, AdapterError>;

    async fn parse_response(
        &self,
        response: reqwest::Response,
        started_at: Instant,
        mode: TestMode,
    ) -> ParsedResponse;
}
```

适配器列表：

```text
AnthropicMessagesAdapter
OpenAiChatAdapter
OpenAiResponsesAdapter
GeminiNativeAdapter
BedrockConverseStreamAdapter (仅返回 UnsupportedAuthentication)
```

Pi 的协议值复用相应通用适配器，但允许 Pi `compat` 修改字段名和 URL 规则。

### 7.1 请求体最小化原则

测试只发送单轮文本请求，不发送工具、图片、文件、历史会话或用户环境信息。

非流式和流式请求必须使用相同模型、相同提示词和尽可能相同的基础参数，唯一差异是 `stream` 和对应响应解析方式。

建议默认输出上限：

```text
max_tokens / max_output_tokens：256
请求总超时：180 秒
连接超时：15 秒
首字节超时：60 秒
流式空闲超时：60 秒
```

这些值放入应用设置，允许用户调整，但不能低于合理的网络超时下限。

### 7.2 URL 规则

必须实现 URL 规范化：

- 去除末尾多余 `/`。
- 解析 URL，限制为 `http` 或 `https`。
- `isFullUrl = true` 时把配置地址当作完整请求地址，不追加路径。
- 否则按协议追加路径。
- 不重复追加 `/v1`。
- 保留配置中的必要路径前缀。
- Gemini 模型 ID 放入路径时必须进行 URL path segment 编码。
- Bedrock 仅生成“未支持认证”结果，不实际构造请求。

默认路径：

```text
Anthropic Messages:
  POST {base}/v1/messages

OpenAI Chat:
  POST {base}/chat/completions

OpenAI Responses:
  POST {base}/responses

Gemini non-stream:
  POST {base}/v1beta/models/{model}:generateContent

Gemini stream:
  POST {base}/v1beta/models/{model}:streamGenerateContent

Bedrock:
  第一版不发送请求
```

### 7.3 Header 优先级

认证 Header 生成后，按以下规则去重和覆盖：

1. HTTP 客户端默认 Header。
2. 协议固定 Header。
3. provider 的字面量自定义 Header（Pi `headers`、Codex `http_headers`）。
4. 根据配置字段生成认证 Header；如果第 3 步已经存在同名认证 Header，保留用户显式 Header，不生成第二个值。
5. `meta.localProxyRequestOverrides.headers` 应用于非保护 Header；保护 Header 见下方。
6. `meta.customUserAgent` 最后覆盖所有 `User-Agent`，非法值忽略并产生配置警告。

保护 Header 不允许被 Header override 改写：

```text
host
content-length
transfer-encoding
connection
proxy-authorization
proxy-authenticate
te
trailer
upgrade
accept-encoding
content-type
authorization
x-api-key
x-goog-api-key
chatgpt-account-id
```

限制：

- Header 名称大小写不敏感地去重。
- 拒绝包含控制字符的 Header 名和值。
- 不允许用户覆盖 `Host`、`Content-Length`、`Transfer-Encoding`。
- 不打印 Authorization、x-api-key、api-key 和疑似密钥 Header。
- `Content-Type` 最终必须与适配器请求体一致。

认证映射：

```text
Anthropic API Key:
  x-api-key: <key>

Anthropic Auth Token:
  Authorization: Bearer <token>

OpenAI/Pi API Key:
  Authorization: Bearer <key>

Gemini API Key:
  `env.GEMINI_API_KEY` -> `x-goog-api-key: <key>`；如果配置中已经提供同名显式 Header，保留显式 Header

Bedrock:
  第一版不支持
```

若某供应商依赖非标准认证 Header，用户必须在 cc-switch 配置的 `headers` 中提供；工具不猜测供应商认证规则。

### 7.4 Body 覆盖

`meta.localProxyRequestOverrides.body` 原本属于 cc-switch 本地代理语义。直测模式默认不应用，避免把代理内部字段带入上游。

提供高级设置：

```text
应用 cc-switch Body 覆盖：关闭（默认）
```

开启后：

- 仅对 JSON object 做深合并。
- 保护 `model`、`stream`、核心消息字段不被覆盖，除非用户明确勾选“允许覆盖核心字段”。
- 在请求详情中显示“已应用 Body 覆盖”。
- 合并失败时任务标记为配置错误，不发送请求。

Header 覆盖默认应用，因为它们通常是供应商必需的请求头。

### 7.5 最小请求契约

所有测试请求都是单轮文本请求。实现必须用结构化 JSON 构造请求，禁止通过字符串拼接 JSON。

Anthropic Messages 非流式/流式：

```json
{
  "model": "<model_id>",
  "max_tokens": 256,
  "messages": [{ "role": "user", "content": "<prompt>" }],
  "stream": false
}
```

Anthropic 必须发送 `anthropic-version`；默认使用 `2023-06-01`，但如果 provider 显式 Header 已指定，则保留显式值。流式请求将 `stream` 改为 `true`。

OpenAI Chat Completions：

```json
{
  "model": "<model_id>",
  "messages": [{ "role": "user", "content": "<prompt>" }],
  "max_tokens": 256,
  "stream": false
}
```

如果 Pi `compat.maxTokensField` 为 `max_completion_tokens`，使用该字段；默认使用 `max_tokens`。如果 `compat.supportsDeveloperRole` 为 false，不生成 developer role。本工具不发送 tools、tool_choice、response_format 等非测试必需字段。

OpenAI Responses：

```json
{
  "model": "<model_id>",
  "input": [{ "role": "user", "content": [{ "type": "input_text", "text": "<prompt>" }] }],
  "max_output_tokens": 256,
  "stream": false,
  "store": false
}
```

如果 provider/compat 明确禁止 `store`，删除该字段；如果配置要求 Responses 原生但不支持 `store`，默认也删除，避免给兼容网关增加无关参数。流式请求将 `stream` 改为 `true`。

Gemini Native：

```json
{
  "contents": [
    { "role": "user", "parts": [{ "text": "<prompt>" }] }
  ],
  "generationConfig": { "maxOutputTokens": 256
  }
}
```

Gemini 使用 `GEMINI_API_KEY` 对应的 `x-goog-api-key` Header，不把 Key 拼入 URL。若 `base_url` 已包含 `/v1beta`，不得重复追加；如果 `isFullUrl` 为 true，按完整 URL 规则处理。

请求方法和模式：

```text
快速非流式：stream=false，读取完整 JSON
兼容性流式：stream=true，按协议读取事件/JSON chunk，必须收到协议完成信号或合法结束标记
```

### 7.6 compat 处理边界

- 只实现本文档明确列出的 compat 字段：`maxTokensField`、`supportsStore`、`supportsDeveloperRole`、`supportsReasoningEffort`、`thinkingFormat`、`requiresReasoningContentOnAssistantMessages`、`deferredToolsMode` 等在请求构造确有语义的字段。
- 未知 compat 字段保留在快照和详情摘要中，但不猜测其含义、不自动发送。
- `maxTokens`、`contextWindow`、`reasoning`、`thinkingLevelMap` 是模型能力信息；单次测活默认不根据它们生成额外长上下文或推理参数。
- 测试提示词不包含工具调用，因此 tool-call 专用 compat 不应改变最小文本请求。

## 8. 流式响应解析

### 8.1 Anthropic

非流式提取：

```text
response.content[*].text
response.stop_reason
response.usage
```

流式提取：

```text
content_block_delta.delta.text
message_delta.stop_reason
message_stop
```

### 8.2 OpenAI Chat

非流式提取：

```text
choices[0].message.content
choices[0].message.reasoning_content
choices[0].finish_reason
usage
```

流式提取：

```text
choices[*].delta.content
choices[*].delta.reasoning_content
[data: [DONE]]
```

### 8.3 OpenAI Responses

非流式提取：

```text
output_text
output[*].content[*].text
status
usage
```

流式提取：

```text
response.output_text.delta
data
response.completed
response.failed
```

解析器不能只依赖某个 SDK 的强类型结构，应允许未知字段存在，并对已知字段按优先级提取。

### 8.4 Gemini

非流式和流式都从以下结构中提取文本：

```text
candidates[*].content.parts[*].text
```

同时解析：

```text
finishReason
promptFeedback
usageMetadata
```

Gemini 流式响应可能是多个 JSON 对象，必须支持逐块解析，不能把整个响应当作单个 JSON。

### 8.5 Bedrock

第一版不执行 Bedrock 请求。若发现 `bedrock-converse-stream`：

```text
状态：skipped
分类：unsupported_authentication
说明：第一版未实现 Bedrock 认证
```

不能将其伪装成网络失败或模型不可用。

### 8.6 流式完成和异常规则

- SSE 按 `event`/`data` 逐事件解析；允许未知事件但必须保留原始事件类型计数，不因未知事件立即失败。
- Anthropic 必须收到 `message_stop`；OpenAI Chat 必须收到 `[DONE]` 或合法的终止 chunk；OpenAI Responses 必须收到 `response.completed` 或 `response.failed`；Gemini 必须收到合法的最终 chunk/HTTP EOF 且没有解析错误。
- 收到文本后连接异常、空闲超时、非法 JSON 或缺少完成信号，统一判为流式失败，不因已有部分文本而成功。
- SSE `data: [DONE]` 只作为结束标记，不作为模型文本；事件内的 error 对象优先于正常文本判定。
- 流式响应读取设置两个独立上限：单次响应最大字节数 8 MiB，累计可提取文本最大字符数 1 MiB；超过则中止并标记 `response_too_large`。
- 非流式响应也设置 8 MiB 上限；超过上限不得继续无限读取。

## 9. 测试提示词

### 9.1 默认提示词

默认内置以下提示词，全部可编辑、禁用、删除和恢复默认：

```text
请用两句话解释 Rust 中 Result 和 Option 的区别。

请给出一个最小 JSON 示例，包含 name 和 version 两个字段。

请说明排查一个偶发 HTTP 500 错误时，最先应该检查的三项内容。

请写一个简单的 Rust 函数，将字符串解析为整数并处理解析失败。

请用一句话说明依赖锁文件在软件项目中的作用。

请说明 HTTP 请求中的幂等性为什么会影响重试策略。

请给出一个 SQL 查询，用于查找 users 表中最近创建的 10 条记录。

请用简短示例说明 JavaScript Promise 中如何处理异常。
```

这些提示词是正常编程问题，不使用 `hello`、`hi`、`test`、`ping` 等简单探测词。工具不声称这些提示词能够规避供应商风控。

### 9.2 随机策略

- 每次尝试随机选择一个已启用提示词。
- 同一模型连续两次尝试不重复提示词，启用提示词少于 2 条时允许重复。
- 随机数由 Rust 生成，记录实际发送的提示词。
- 不添加固定“测试成功”字样作为成功条件，避免把模型输出限制在固定格式。
- 可选校验标记功能默认关闭；开启后，在提示词末尾追加每次随机生成的短标记，并验证原始输出是否包含该标记。

### 9.3 提示词编辑规则

字段：

```text
id
name
content
enabled
created_at
updated_at
```

校验：

- name 非空，最多 80 字符。
- content 非空，最多 2000 字符。
- 删除默认提示词前要求确认。
- 全部提示词禁用时禁止开始测试。
- 恢复默认不删除用户自定义提示词，只恢复缺失的默认项。

## 10. 调度器

### 10.1 任务展开和自动去重

用户选中供应商和模型后，先展开原始目标：

```text
Provider × Endpoint × Model × Attempt
```

然后执行第 5.8 节定义的精确去重。去重必须发生在计算实际请求数、创建测试批次和发送任何请求之前。

例如：

```text
去重前：3 个来源 × 1 个端点 × 2 个模型 × 3 次 = 18 个 attempt
其中 4 个 attempt 目标完全重复
去重后：14 个 attempt
```

实现要求：

- 去重过程是确定性的，同样的配置和选择顺序必须得到同样的代表项。
- 去重不是“测试一个供应商后跳过同名供应商”；每个实际不同的 endpoint/model/protocol/credential/header 组合都必须保留。
- 去重结果要同时返回 `original_target_count`、`deduplicated_target_count`、`original_attempt_count`、`deduplicated_attempt_count`、`duplicate_group_count` 和 `removed_attempt_count`。
- 只有用户在确认对话框中同意后，才创建正式 `test_runs` 并进入调度器。
- 被合并来源不创建假的成功或失败 attempt，避免成功率被重复放大。
- 去重后的代表目标携带 `source_refs`；历史只记录来源的应用、provider ID、名称和模型 key，不记录认证信息。
- 代表项失败时，详情中显示该结果代表多个 cc-switch 来源；不能把代表结果伪装成每个来源都各自请求过。

同一个模型在不同供应商下通常是不同测试对象；只有第 5.8 节的精确指纹完全一致时才合并。同一供应商的不同模型共享供应商并发限制。

### 10.2 并发限制

使用两级信号量：

```text
global_semaphore = 10
provider_semaphore[(app_type, provider_id)] = 1
```

获取顺序：

1. 获取全局许可。
2. 获取供应商许可。
3. 等待供应商最小请求间隔。
4. 发送请求。
5. 完成、失败或取消后释放两级许可。

用户可修改：

```text
全局并发：1-50，默认 10
单供应商并发：1-10，默认 1
```

为了避免误用，首次把全局并发调到 10 以上时显示风险确认。工具不是压力测试器。

### 10.3 重试、限流和取消

- 不自动重试任何失败。
- 429 记录为 `rate_limited`，不自动重试。
- 可配置同一供应商请求最小间隔，默认 0 秒；随机提示词是默认降低重复探测特征的机制。
- 取消后不再发送排队任务。
- 正在进行的请求调用 Tokio cancellation token，中止读取响应。
- 用户关闭窗口时，若存在活动测试，要求确认；确认后取消并等待任务清理。
- 暂停只阻止新任务开始，当前 HTTP 请求继续完成；取消才中止当前请求。

### 10.4 测试批次状态

```text
created
running
paused
cancelling
completed
cancelled
failed
```

单次 attempt 状态：

```text
queued
running
success
failed
skipped
cancelled
```

## 11. 可用性判定

### 11.1 判定顺序

```text
请求配置校验
    ↓
网络请求
    ↓
HTTP 状态
    ↓
协议解析
    ↓
响应内容提取
    ↓
负面规则匹配
    ↓
成功/失败判定
```

### 11.2 状态分类

```text
success
unstable
unavailable
configuration_error
authentication_failed
rate_limited
network_error
protocol_error
empty_response
response_too_large
negative_match
unsupported_authentication
cancelled
```

单次 attempt 不显示 `unstable`，只能显示成功或具体失败原因。`unstable` 是模型汇总状态。

### 11.3 HTTP 分类

```text
2xx：继续解析，不能直接判定成功
401/403：authentication_failed
404：unavailable 或 configuration_error，依据错误内容区分
408：network_error
429：rate_limited
500-599：unavailable
其他 4xx：configuration_error 或 unavailable
```

### 11.4 成功条件

同时满足：

1. 请求成功收到完整响应。
2. HTTP 状态为 2xx。
3. 响应符合对应协议结构。
4. 提取到非空模型文本。
5. 没有协议级错误对象。
6. 没有命中启用的失败规则。
7. 如果开启校验标记，输出包含本次标记。

即使 HTTP 200，也必须在以下任一情况判失败：

- body 含 `error` 对象。
- 输出为空。
- JSON/SSE 解析失败。
- 流式中途断开。
- 命中 `api error`、`model not found` 等规则。

### 11.5 默认负面规则

```text
api error
internal server error
invalid api key
authentication failed
model not found
quota exceeded
insufficient quota
request rejected
```

规则字段：

```text
id
pattern
match_type: contains | regex
scope: raw_response | extracted_text | error_fields
case_sensitive
enabled
action: fail
```

默认规则可编辑、禁用、删除、恢复。正则编译失败时保存失败并提示，不允许运行时静默忽略。

## 12. 稳定性和延迟指标

### 12.1 稳定性统计

每个 `app + provider_id + endpoint_url + model_id + protocol + mode` 单独聚合。相同模型跨供应商、跨端点或跨模式不能直接合并成一个成功率。

默认 N=3，用户可修改。汇总规则：

```text
3/3 成功：稳定可用
1/3 或 2/3 成功：不稳定
0/3 成功：不可用
```

通用规则是：全部已发送尝试成功则稳定可用，部分成功则不稳定，全部已发送尝试失败则不可用。

- `success_rate = success_count / sent_attempt_count`。
- 取消、跳过、配置错误和暂不支持且未发送的 attempt 不计入分母；它们单独显示原因。
- 已发送但因网络、HTTP、协议或内容判定失败的 attempt 计入分母和失败次数。
- 如果没有任何实际发送的 attempt，成功率为空，不显示 0%。
- 被取消的运行只能显示“未完成”，不生成最终稳定性结论。
- “稳定可用”只代表本批次 N 次请求全部成功，不等同长期 SLA 或模型质量保证。
- 多个候选端点开启测试后，每个端点生成独立汇总行；可以另显示供应商总览，但不能隐藏端点差异。

### 12.2 延迟指标

每次 attempt 记录：

```text
started_at
connection_ms（能可靠取得时）
first_byte_ms
first_text_ms
total_latency_ms
stream_idle_timeout
response_chars
```

定义：

- `first_byte_ms`：收到第一个响应字节的耗时。
- `first_text_ms`：收到第一个可提取文本片段的耗时。
- `total_latency_ms`：完整响应解析完成的耗时。
- 非流式没有有效 token 片段时，`first_text_ms` 为空。
- 流式中途断开不计为成功，即使已经收到部分文本。

## 13. 本地数据存储

工具数据库路径：

```text
%LOCALAPPDATA%\CcSwitchModelTester\data.db
```

推荐表：

```sql
CREATE TABLE app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE prompt_templates (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  content TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  is_builtin INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE negative_rules (
  id TEXT PRIMARY KEY,
  pattern TEXT NOT NULL,
  match_type TEXT NOT NULL,
  scope TEXT NOT NULL,
  case_sensitive INTEGER NOT NULL DEFAULT 0,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE test_runs (
  run_id TEXT PRIMARY KEY,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  status TEXT NOT NULL,
  total_attempts INTEGER NOT NULL,
  completed_attempts INTEGER NOT NULL DEFAULT 0,
  original_target_count INTEGER NOT NULL,
  deduplicated_target_count INTEGER NOT NULL,
  original_attempt_count INTEGER NOT NULL,
  deduplicated_attempt_count INTEGER NOT NULL,
  duplicate_group_count INTEGER NOT NULL DEFAULT 0,
  removed_attempt_count INTEGER NOT NULL DEFAULT 0,
  dedup_summary_json TEXT NOT NULL DEFAULT '{}',
  config_snapshot_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE test_attempts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  app_type TEXT NOT NULL,
  provider_id TEXT NOT NULL,
  provider_name TEXT NOT NULL,
  model_id TEXT NOT NULL,
  model_display_name TEXT,
  protocol TEXT NOT NULL,
  mode TEXT NOT NULL,
  endpoint_url TEXT NOT NULL,
  endpoint_url_hash TEXT NOT NULL,
  dedup_key_hash TEXT NOT NULL,
  duplicate_source_count INTEGER NOT NULL DEFAULT 1,
  source_refs_json TEXT NOT NULL DEFAULT '[]',
  attempt_no INTEGER NOT NULL,
  prompt_text TEXT NOT NULL,
  tested_at INTEGER NOT NULL,
  status TEXT NOT NULL,
  category TEXT NOT NULL,
  http_status INTEGER,
  first_byte_ms INTEGER,
  first_text_ms INTEGER,
  total_latency_ms INTEGER,
  response_chars INTEGER NOT NULL DEFAULT 0,
  response_summary TEXT,
  error_summary TEXT,
  matched_rule TEXT,
  endpoint_display TEXT,
  FOREIGN KEY (run_id) REFERENCES test_runs(run_id) ON DELETE CASCADE
);
```

约束：

- `test_attempts` 不保存完整响应正文。
- 不保存 API Key、Auth Token 或完整原始 Header。
- `response_summary` 最多 4096 字符，写入前执行密钥和 Authorization 脱敏。
- `endpoint_display` 删除 URL 查询参数中的疑似密钥；另外保存不可逆的 `endpoint_url_hash` 用于相同端点聚合。
- 当前活动测试的完整响应只存在 Rust 内存缓存，详情查看后可释放。
- 删除历史批次时级联删除 attempt。
- 为减少资源占用，默认保留最近 30 天历史；保留期限做成设置项，设置为 0 表示不自动清理。
- 每次启动执行轻量清理，不做大规模 vacuum 阻塞 UI。

## 14. Tauri 命令和事件

### 14.1 Commands

```text
get_app_info()

get_ccswitch_source()
set_ccswitch_source(path)
validate_ccswitch_source(path)

load_provider_catalog(app_type)
reload_provider_catalog(app_type)

get_app_settings()
save_app_settings(settings)

list_prompt_templates()
create_prompt_template(input)
update_prompt_template(input)
delete_prompt_template(id)
restore_builtin_prompts()

list_negative_rules()
create_negative_rule(input)
update_negative_rule(input)
delete_negative_rule(id)
restore_builtin_negative_rules()

validate_proxy(proxy_url)
save_proxy(proxy_url)

preview_test(input) -> DedupPreview
start_test(preview_id) -> run_id
pause_test(run_id)
resume_test(run_id)
cancel_test(run_id)
get_run_status(run_id)
get_active_attempt_detail(run_id, attempt_id)
release_active_run_details(run_id)

query_test_history(filter)
get_test_run_summary(run_id)
delete_test_run(run_id)
```

Rust Command 输入对象不能包含 API Key。先调用 `preview_test`，测试开始时传：

```json
{
  "app": "claude",
  "providerIds": ["provider-a"],
  "modelKeys": ["provider-a:model-x"],
  "attemptsPerModel": 3,
  "mode": "streaming",
  "globalConcurrency": 10,
  "providerConcurrency": 1,
  "testAllCandidateEndpoints": false,
  "applyBodyOverrides": false
}
```

- `preview_test` 只做目标展开、配置校验和去重，不创建 `test_runs`，不产生网络请求，也不写历史。
- `start_test(preview_id)` 必须检查预览未过期，并重新确认配置快照哈希、提示词策略和测试参数未变化；变化时返回 `preview_expired_or_changed`，要求前端重新预览。
- 没有重复时，前端也必须经过 `preview_test`，随后自动调用 `start_test`，保证请求总数显示准确。
- 用户在重复确认框点击取消时不创建运行记录；只提示“已取消，未发送请求”。

### 14.2 Events

事件名称：

```text
catalog-loaded
catalog-warning
test-run-created
test-attempt-started
test-attempt-progress
test-attempt-finished
test-run-progress
test-run-paused
test-run-cancelled
test-run-finished
```

`test-run-created` 的 payload 必须包含去重摘要：

```json
{
  "runId": "...",
  "originalTargetCount": 6,
  "deduplicatedTargetCount": 4,
  "originalAttemptCount": 18,
  "deduplicatedAttemptCount": 12,
  "duplicateGroupCount": 2,
  "removedAttemptCount": 6
}
```

`test-attempt-finished` 只发送脱敏摘要，不发送 API Key 和完整供应商配置。

示例：

```json
{
  "runId": "...",
  "attemptId": 12,
  "providerId": "provider-a",
  "modelId": "model-x",
  "endpointUrlHash": "...",
  "dedupKeyHash": "...",
  "duplicateSourceCount": 2,
  "sourceRefs": [
    { "app": "claude", "providerId": "provider-a", "providerName": "Provider A", "modelKey": "provider-a:model-x" },
    { "app": "codex", "providerId": "provider-b", "providerName": "Provider B", "modelKey": "provider-b:model-x" }
  ],
  "attemptNo": 2,
  "status": "success",
  "category": "success",
  "httpStatus": 200,
  "firstTextMs": 820,
  "totalLatencyMs": 2450,
  "responseChars": 118,
  "responseSummary": "Rust 中 Result 用于表示成功或失败..."
}
```

## 15. UI 规格

### 15.1 主布局

```text
顶部：数据源状态、重新加载、设置入口
主区域：Claude Code | Codex | Pi agent
每个标签页：
  左侧：供应商和模型选择
  中间：测试参数和提示词设置
  下方：进度和结果表
右侧或抽屉：当前运行详情
```

### 15.2 供应商和模型选择

必须支持：

- 供应商多选。
- 模型多选。
- 按名称和 ID 搜索。
- 协议筛选。
- 状态筛选：可测试、配置错误、暂不支持、托管认证跳过。
- 全选当前筛选结果。
- 反选当前筛选结果。
- 显示每个供应商的模型数和端点数。
- 显示凭据状态，但只显示 `已配置`、`未配置`、`托管认证`，不显示密钥。
- 供应商列表可显示“可能重复”标记，但这只是基于粗粒度信息的提醒，不在列表加载阶段自动合并。
- 开始测试后如果发现精确重复目标，显示去重确认对话框；没有重复时不显示该对话框。
- 重复对话框显示每组代表目标、被合并的应用/供应商/模型、去重前后目标数和请求数。
- 结果表中代表项显示“来源 N 个”标记；点击后可查看被合并来源，但不显示其 API Key。
- 被合并来源不产生虚假的独立结果行；汇总行明确标记“实际发送 1 组请求”。

### 15.3 测试控制

字段：

```text
测试次数：1-20，默认 3
测试模式：快速非流式 / 兼容性流式
全局并发：1-50，默认 10
单供应商并发：1-10，默认 1
代理：默认 socks5://127.0.0.1:1080
测试全部候选端点：关闭
应用 Body 覆盖：关闭
校验标记：关闭
请求超时：默认 180 秒
```

按钮：

```text
开始测试
暂停
继续
取消
重新加载配置
打开提示词管理
打开负面规则管理
打开测试历史
```

开始测试前的确认框显示：

```text
即将发送 N 个真实模型请求，可能产生费用并消耗供应商额度。
当前使用代理：...
当前模式：...
```

没有选中任何目标、没有启用提示词、存在全部目标均不支持或配置错误时，禁止启动并给出原因。

### 15.4 结果表

默认列：

```text
状态
应用
供应商
模型
协议
测试模式
成功率
平均延迟
首字节延迟
HTTP 状态
最近测试时间
```

支持按所有可见表头排序、分页、列显示设置和筛选。

点击模型汇总行展开 attempt：

```text
第几次
状态
提示词
HTTP 状态
首字节耗时
完整耗时
错误摘要
命中规则
```

点击活动 attempt 的“详情”：

```text
完整发送提示词
请求 URL（脱敏）
请求 Header（脱敏）
请求体摘要
完整模型响应（仅内存）
原始错误信息
解析过程摘要
```

历史 attempt 只能显示持久化的摘要；完整响应不存在时应明确显示“完整响应未持久化”。

## 16. 错误和用户提示

所有错误必须同时包含机器分类和用户可读说明，例如：

```text
分类：authentication_failed
说明：供应商返回 HTTP 401，配置中的认证信息可能无效或没有该模型权限。
```

不要把以下内容直接显示给用户：

- 完整 API Key。
- 完整 Authorization Header。
- 未脱敏的 URL 查询参数。
- Rust panic 或内部栈回溯。

必须区分：

```text
cc-switch 数据库不存在
数据库版本不支持
供应商配置 JSON 无效
模型列表为空
缺少 Base URL
缺少认证信息
托管 OAuth 已跳过
协议暂不支持
代理连接失败
DNS/连接/TLS 失败
请求超时
HTTP 认证失败
HTTP 限流
上游服务错误
协议解析失败
模型返回空内容
命中负面规则
用户取消
```

## 17. 性能和资源约束

目标：

- 空闲时不轮询供应商，不发送后台请求。
- 只有用户点击重新加载或开始测试时读取配置。
- 使用一个共享 reqwest Client 和连接池。
- 流式响应按块处理，不复制完整响应多份。
- 摘要和详情使用长度上限。
- UI 结果表使用虚拟列表或分页，不能一次渲染数万行。
- 数据库查询全部分页，禁止无限制查询历史。
- 限制单次加载供应商配置和单个响应体大小，超限给出配置/响应过大提示。
- 测试完成后主动释放明文凭据和完整响应缓存。
- 不使用轮询式进度刷新，使用 Tauri event 推送。

建议验收目标：

```text
无测试任务时 CPU 接近 0
100 个模型目标只加载摘要，不因为 UI 渲染造成明显卡顿
10 并发任务下 UI 可操作
取消任务后 5 秒内停止新增请求
```

具体内存数字应通过 Windows Release 构建实测后确定，不要在未测量前承诺固定 MB 数值。

## 18. 安全和隐私

- cc-switch 数据库以只读方式打开。
- API Key 只在 Rust 请求生命周期内使用。
- 不写入工具日志、崩溃报告、前端状态或历史数据库。
- 日志统一经过 `redact_secret()`。
- 响应摘要执行常见密钥格式脱敏，包括 Bearer、x-api-key、sk- 前缀和 JSON key 字段。
- 禁止遥测、自动更新检查和远程配置拉取，除非未来另行批准。
- 代理配置可以保存，但不能在错误消息中回显代理认证密码。
- 提示词和响应可能包含用户代码；只在本机保存提示词和摘要。
- 不自动切换直连，避免用户以为代理仍然生效。

## 19. 测试计划

### 19.1 单元测试

必须覆盖：

- SQLite schema 18 识别。
- providers 查询和 JSON 解析。
- Claude 各模型字段提取和去重。
- Codex TOML provider 表和 `experimental_bearer_token` 提取。
- Codex 外部 model catalog 缺失处理。
- Pi provider/model/baseUrl/compat 合并。
- URL 规范化和 `isFullUrl`。
- Header 大小写去重和敏感信息脱敏。
- 四类实际请求协议的请求体构造：Anthropic Messages、OpenAI Chat、OpenAI Responses、Gemini Native。
- Pi 的协议枚举映射，以及 Bedrock `unsupported_authentication` 跳过分支。
- JSON、SSE、Gemini chunk 响应解析。
- 200 + error object 判失败。
- 200 + 空输出判失败。
- 200 + 超出响应大小上限判失败且不保存超限正文。
- 200 + 负面词判失败。
- 401、403、404、429、5xx 分类。
- 流式中途断开判失败。
- 全局并发不超过设定值。
- 单供应商并发不超过设定值。
- 每个模型准确执行 N 次。
- 相同 endpoint、协议、模型、模式、认证和有效请求 Header 的目标只实际执行一次 N 次；结果携带全部来源引用。
- 相同名称但不同 endpoint 或 API Key 的供应商不被错误合并。
- 主端点与候选端点 URL 规范化后相同才去重，不同端点仍分别测试。
- 去重预览只读、不发请求、不创建历史；确认前取消不产生测试记录。
- 配置快照在预览后发生变化时，启动被拒绝并要求重新预览。
- 去重数量和预计减少请求数量在确认框、运行事件和历史记录中一致。
- 暂停、继续、取消状态转换。
- API Key 不出现在任何持久化对象和日志中。

### 19.2 Mock Server 集成测试

创建 Rust 测试用 Mock Server，不调用真实供应商：

- Anthropic non-stream success。
- Anthropic stream success。
- OpenAI Chat non-stream success。
- OpenAI Chat stream success。
- OpenAI Responses non-stream success。
- OpenAI Responses stream success。
- Gemini non-stream success。
- Gemini stream success。
- 200 返回 API error。
- 200 返回空 choices/content。
- SSE 格式错误。
- Gemini 多 JSON chunk。
- 连接超时和流式 idle timeout。
- 代理连接成功和代理连接失败。
- 自定义 Header 到达 Mock Server。
- User-Agent 覆盖顺序正确。
- Body override 默认不生效，开启后按规则生效。

### 19.3 UI 测试

- 三标签页切换不丢失各自筛选状态。
- 供应商/模型多选正确展开任务数。
- 相同测试目标自动去重，重复确认框显示代表项、来源项和减少请求数。
- 不同 API Key、协议、模式、有效 Header 或端点的目标不会被错误去重。
- 跨标签页去重仅在共享测试批次启用时发生；独立标签页批次不互相影响。
- 提示词增删改和启停。
- 结果表排序和筛选。
- 活动任务详情显示完整响应。
- 历史详情显示摘要且不假装存在完整响应。
- 暂停、继续、取消按钮状态正确。
- 关闭窗口的活动任务确认。
- 代理错误有明确提示。

### 19.4 Windows 验证

在 Windows Release 构建上验证：

- Windows 10 x64。
- Windows 11 x64。
- SOCKS5 代理。
- 中文路径。
- cc-switch 路径不可访问。
- cc-switch 正在运行时读取数据库。
- 便携版运行。
- MSI 安装和卸载。
- 应用退出后没有残留测试进程。

## 20. 开发里程碑

### M1：项目骨架和安全边界

交付：

- Tauri 2 + Svelte 工程。
- Windows 构建。
- 本地日志和错误框架。
- 自有 SQLite 初始化。
- cc-switch 数据库只读连接。
- 不包含真实网络请求。

### M2：配置解析和统一目录

交付：

- Claude、Codex、Pi 解析器。
- schema 18 校验。
- 托管认证过滤。
- 统一 ProviderSnapshot/ModelSnapshot。
- 三标签页供应商和模型选择。
- 脱敏测试数据 fixture。

### M3：协议请求和响应解析

交付：

- Anthropic Messages。
- OpenAI Chat。
- OpenAI Responses。
- Gemini Native。
- Pi 协议映射。
- 非流式和流式模式。
- Mock Server 集成测试。
- Bedrock 显示为暂不支持认证。

### M4：调度器和判定引擎

交付：

- 每模型 N 次。
- 全局/供应商两级并发。
- 暂停、继续、取消。
- 提示词随机策略。
- 负面规则。
- 结果分类和稳定性汇总。

### M5：历史、详情和完整 UI

交付：

- 本地历史摘要。
- 运行期间详情缓存。
- 结果排序、筛选、分页。
- 提示词管理界面。
- 负面规则管理界面。
- 代理设置和候选端点选项。

### M6：安全审查和 Windows 发布

交付：

- Key 泄露扫描。
- Release 构建。
- MSI 和 Portable。
- Windows 10/11 验证。
- 用户手册。
- 已知限制说明。
- 全部自动化测试通过。

## 21. 最终验收标准

以下条件全部满足才能交付：

1. 能读取 cc-switch 3.20.1 schema 18 数据库。
2. Claude、Codex、Pi 三个标签页都能显示对应供应商和模型。
3. 至少一个脱敏 fixture 能覆盖每个配置解析分支。
4. 四类实际测试协议均支持非流式和流式：Anthropic、OpenAI Chat、OpenAI Responses、Gemini Native。
5. Pi 的三类对应协议能正确复用适配器，Bedrock 明确跳过且不发送请求。
6. 每个模型严格执行配置的测试次数。
7. 全局并发和单供应商并发均不超限。
8. 200 + error、200 + 空响应、200 + 超出响应上限、200 + 负面词均判为失败。
9. 失败不自动重试。
10. 代理失败不会回退直连。
11. 测试提示词默认不使用 hello/hi/test/ping，并可增删改。
12. 历史只包含摘要，不包含完整响应和明文 Key。
13. 活动测试可以查看完整响应，退出后完整响应释放。
14. 取消操作能停止排队任务和正在进行的 HTTP 请求。
15. Windows Release 包可安装或直接运行。
16. 测试、日志、错误提示中找不到明文 API Key。
17. 工具不会写入 cc-switch 数据库或其任何 live 配置文件。
18. 用户能够明确区分成功、稳定、不稳定、认证失败、限流、网络错误、协议错误和暂不支持。
19. 同一测试批次内完全相同的测试目标自动去重，并在发送请求前提示去重明细和减少的请求数。
20. 去重只合并精确等价的请求目标，不因供应商名称、provider ID 或域名相同而合并。
21. 去重后的代表结果保留被合并来源，但不伪造来源的独立请求记录。
22. 预览/确认阶段不发送网络请求，配置发生变化时必须重新预览。

## 22. 开发模型执行要求

第三方开发模型开始编码前必须先：

1. 阅读本文档和 `Requirement.md`。
2. 阅读第 3 节列出的 cc-switch v3.20.1 源码链接。
3. 创建脱敏 fixture，不读取开发机真实 API Key。
4. 先实现 Mock Server 和协议解析测试，再接入真实 HTTP。
5. 每个里程碑完成后运行对应测试并报告变更文件。
6. 不得为了“兼容更多供应商”读取系统环境变量或修改 cc-switch 配置。
7. 遇到 cc-switch 字段与本文档不一致时，停止并报告具体字段、版本和链接，不要静默猜测。
8. 不得把简单 GET `/models` 或 GET `/health` 的结果当作模型可用性测试成功。
9. 不得把 HTTP 200 直接当作成功。
10. 不得将 Bedrock 未实现认证伪装成已支持。

## 23. 去重范围待确认

当前产品结构是 Claude Code、Codex、Pi agent 三个独立标签页。本文档默认每个标签页的一次“开始测试”创建一个独立测试批次，因此只在当前标签页内自动去重；分别点击三个标签页的“开始测试”时，不跨批次合并，也不因历史上测试过相同端点而跳过请求。

如果需要跨标签页去重，则必须增加“跨应用统一测试批次”入口，至少需要：

- 允许用户在一个批次中同时选择三个标签页的供应商和模型。
- 将 `TestRunInput.app` 改为 `apps` 或改为按应用分组的选择结构。
- 在一个预览页显示三类应用的全部原始目标、去重组和请求数量。
- 明确相同端点/模型但不同协议或模式是否仍然分别测试；本文档的精确去重规则默认要求协议和模式也相同。

请确认采用以下哪一种：

```text
A. 保持三个标签页独立批次，只在当前标签页内去重（当前默认方案，改动较小）
B. 增加跨标签页统一测试批次，三个标签页可以合并选择并跨应用去重（功能更完整，但需要调整 UI 和命令模型）
```
