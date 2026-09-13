// 临时诊断 6：1c794064（api.zzzcoding.org copy / claude）——1M 与 tools gate 定位（不入仓库）
import { DatabaseSync } from 'node:sqlite';
import { createHash, randomUUID } from 'node:crypto';
import { hostname, homedir } from 'node:os';
import { join } from 'node:path';

const db = new DatabaseSync(join(homedir(), '.cc-switch', 'cc-switch.db'), { readOnly: true });
const row = db.prepare("SELECT id, name, settings_config FROM providers WHERE id LIKE '1c794064%'").get();
const env = JSON.parse(row.settings_config).env;
const baseUrl = (env.ANTHROPIC_BASE_URL ?? '').replace(/\/+$/, '');
const token = env.ANTHROPIC_AUTH_TOKEN ?? env.ANTHROPIC_API_KEY;
console.log(`供应商 ${row.name} | ${baseUrl}`);
console.log('全部模型 env:', Object.entries(env).filter(([k]) => k.includes('MODEL')).map(([k, v]) => `${k}=${v}`).join(' | '));

const deviceId = createHash('sha256').update(`pi-client-emulation:${hostname()}`).digest('hex');
const sessionId = randomUUID();
const identity = 'You are a Claude agent, built on Anthropic' + String.fromCharCode(39) + 's Claude Agent SDK.';

function mkBody(model, { emu = true, tools = false } = {}) {
  const b = { model, max_tokens: 64, stream: false, messages: [{ role: 'user', content: 'hi' }] };
  if (emu) {
    b.system = [{ type: 'text', text: identity }, { type: 'text', text: '请回复 pong' }];
    b.metadata = { user_id: JSON.stringify({ device_id: deviceId, account_uuid: '', session_id: sessionId }) };
  }
  if (tools) {
    b.tools = [
      { name: 'Bash', description: '执行 shell 命令', input_schema: { type: 'object', properties: { command: { type: 'string' } }, required: ['command'] } },
      { name: 'Read', description: '读文件', input_schema: { type: 'object', properties: { path: { type: 'string' } }, required: ['path'] } },
    ];
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
  const t0 = Date.now();
  const res = await fetch(baseUrl + '/v1/messages', { method: 'POST', headers: h, body: JSON.stringify(mkBody(model, { emu, tools })) });
  const text = await res.text();
  console.log(`[${label}] HTTP ${res.status} (${Date.now() - t0}ms) ${res.status !== 200 ? text.slice(0, 120).replace(/\s+/g, ' ') : '✅ ' + text.slice(8, 55)}`);
}

const opusBare = (env.ANTHROPIC_MODEL ?? 'claude-opus-4-7[1M]').replace(/\[[^\]]+\]/g, '');
const fableRaw = env.ANTHROPIC_DEFAULT_FABLE_MODEL ?? 'claude-fable-5-1';
const fableBare = fableRaw.replace(/\[[^\]]+\]/g, '');

await attempt('opus bare+仿真 无beta      ', opusBare, { beta: null });
await attempt('opus bare+仿真 +1m beta     ', opusBare, { beta: 'context-1m-2025-08-07' });
await attempt('opus bare+仿真+tools        ', opusBare, { beta: null, tools: true });
await attempt('opus bare+仿真+1m+tools     ', opusBare, { beta: 'context-1m-2025-08-07', tools: true });
await attempt('fable bare+仿真 无beta      ', fableBare, { beta: null });
await attempt('fable bare+仿真+tools       ', fableBare, { beta: null, tools: true });
await attempt('fable bare+仿真+1m+tools    ', fableBare, { beta: 'context-1m-2025-08-07', tools: true });
await attempt('fable bare 无仿真           ', fableBare, { beta: null, emu: false });
db.close();
