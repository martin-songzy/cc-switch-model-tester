// 临时诊断 4：针对 claude 标签页的 zzzcoding（copy）供应商——1M 启用方式 + tools gate（不入仓库）
import { DatabaseSync } from 'node:sqlite';
import { createHash, randomUUID } from 'node:crypto';
import { hostname, homedir } from 'node:os';
import { join } from 'node:path';

const db = new DatabaseSync(join(homedir(), '.cc-switch', 'cc-switch.db'), { readOnly: true });
const rows = db.prepare("SELECT id, name, settings_config, meta FROM providers WHERE app_type='claude'").all();
// 用户说的是 claude 标签页的 api.zzzcoding.org copy
const row = rows.find((r) => r.name.includes('copy') && r.settings_config.includes('zzzcoding'))
  ?? rows.find((r) => r.name.includes('copy') && r.settings_config.includes('1c794064'))
  ?? rows.filter((r) => r.settings_config.includes('zzzcoding'))[1];
if (!row) { console.log('未找到 copy 供应商'); process.exit(1); }
const env = JSON.parse(row.settings_config).env;
const baseUrl = (env.ANTHROPIC_BASE_URL ?? '').replace(/\/+$/, '');
const token = env.ANTHROPIC_AUTH_TOKEN ?? env.ANTHROPIC_API_KEY;
console.log(`供应商: ${row.name} (${row.id.slice(0, 8)}) | ${baseUrl}`);
console.log('模型 env:', JSON.stringify({
  MODEL: env.ANTHROPIC_MODEL, SONNET: env.ANTHROPIC_DEFAULT_SONNET_MODEL, OPUS: env.ANTHROPIC_DEFAULT_OPUS_MODEL, FABLE: env.ANTHROPIC_DEFAULT_FABLE_MODEL,
}));

const deviceId = createHash('sha256').update(`pi-client-emulation:${hostname()}`).digest('hex');
const identity = 'You are a Claude agent, built on Anthropic' + String.fromCharCode(39) + 's Claude Agent SDK.';
const sessionId = randomUUID();

function mkBody(model, { emu = true, tools = false } = {}) {
  const b = { model, max_tokens: 64, stream: false, messages: [{ role: 'user', content: 'hi' }] };
  if (emu) {
    b.system = [{ type: 'text', text: identity }, { type: 'text', text: '请回复 pong' }];
    b.metadata = { user_id: JSON.stringify({ device_id: deviceId, account_uuid: '', session_id: sessionId }) };
  }
  if (tools) {
    b.tools = [{ name: 'Bash', description: '执行命令', input_schema: { type: 'object', properties: { command: { type: 'string' } }, required: ['command'] } }];
  }
  return b;
}

const cc = {
  'user-agent': 'claude-cli/2.1.252 (external, sdk-cli)',
  'x-app': 'cli',
  'anthropic-version': '2023-06-01',
  'anthropic-dangerous-direct-browser-access': 'true',
  authorization: `Bearer ${token}`,
};

async function attempt(label, model, { beta = null, tools = false, emu = true } = {}) {
  const h = { ...cc, 'content-type': 'application/json' };
  const betas = ['claude-code-20250219'];
  if (beta) betas.push(beta);
  h['anthropic-beta'] = betas.join(',');
  const res = await fetch(baseUrl + '/v1/messages', { method: 'POST', headers: h, body: JSON.stringify(mkBody(model, { emu, tools })) });
  const text = await res.text();
  console.log(`[${label}] HTTP ${res.status} ${res.status !== 200 ? text.slice(0, 110).replace(/\s+/g, ' ') : '✅ ' + text.slice(8, 50)}`);
}

// 从 env 找 4.7 与 fable 的模型 id（剥标记前后）
const opusRaw = env.ANTHROPIC_MODEL ?? '';
const opusBare = opusRaw.replace(/\[[^\]]+\]/g, '');
const fableRaw = env.ANTHROPIC_DEFAULT_FABLE_MODEL ?? '';
const fableBare = fableRaw.replace(/\[[^\]]+\]/g, '');
console.log(`\nopus: raw=${opusRaw} bare=${opusBare}`);
console.log(`fable: raw=${fableRaw} bare=${fableBare}\n`);

await attempt('opus bare 无beta ', opusBare, { beta: null });
await attempt('opus bare +1m beta ', opusBare, { beta: 'context-1m-2025-08-07' });
await attempt('opus raw[1M]命名 ', opusRaw, { beta: null });
await attempt('opus bare+1m+tools', opusBare, { beta: 'context-1m-2025-08-07', tools: true });
await attempt('fable bare 无beta  ', fableBare, { beta: null });
await attempt('fable bare +1m beta ', fableBare, { beta: 'context-1m-2025-08-07' });
await attempt('fable bare +tools   ', fableBare, { beta: null, tools: true });
await attempt('fable bare+1m+tools ', fableBare, { beta: 'context-1m-2025-08-07', tools: true });
db.close();
