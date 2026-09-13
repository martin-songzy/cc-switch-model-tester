// 临时诊断 7：布尔化确认 + opus-4-7 补测（不入仓库）
import { DatabaseSync } from 'node:sqlite';
import { createHash, randomUUID } from 'node:crypto';
import { hostname, homedir } from 'node:os';
import { join } from 'node:path';

const db = new DatabaseSync(join(homedir(), '.cc-switch', 'cc-switch.db'), { readOnly: true });

// 1) claude 的 1c794064 布尔确认
const row = db.prepare("SELECT name, settings_config FROM providers WHERE id LIKE '1c794064%'").get();
const env = JSON.parse(row.settings_config).env;
const base = (env.ANTHROPIC_BASE_URL ?? '').replace(/\/+$/, '');
const token = env.ANTHROPIC_AUTH_TOKEN ?? env.ANTHROPIC_API_KEY;
console.log('[bool] name==="api.zzzcoding.org copy" →', row.name === 'api.zzzcoding.org copy');
console.log('[bool] name==="Any"+"router"        →', row.name === 'Any' + 'router');
console.log('[bool] base 含 "zzzcoding"          →', /zzzcoding/.test(base));
console.log('[bool] base 含 "any"                →', /any/i.test(base));
console.log('[bool] base==="https://api.zzzcoding.org" →', base === 'https://api.zzzcoding.org');

// 2) pi 标签页全部供应商
const piRows = db.prepare("SELECT id, name, settings_config FROM providers WHERE app_type='pi'").all();
console.log('\npi 供应商布尔表:');
for (const r of piRows) {
  let b = '?', models = '?', nameIsTarget = r.name === 'Any' + 'router';
  try {
    const sc = JSON.parse(r.settings_config);
    b = sc.baseUrl ?? '(从 models/settings 推断)';
    models = (sc.models || []).map((m) => m.id).join(', ');
  } catch {}
  console.log(`  id=${r.id} | name匹配目标=${nameIsTarget} | name长度=${r.name.length} | base含zzzcoding=${/zzzcoding/.test(b)} base含any=${/any/i.test(b)} | models=${models}`);
}

// 3) opus-4-7 补测（claude 协议 /v1/messages）
const deviceId = createHash('sha256').update(`pi-client-emulation:${hostname()}`).digest('hex');
const sessionId = randomUUID();
const identity = 'You are a Claude agent, built on Anthropic' + String.fromCharCode(39) + 's Claude Agent SDK.';
function mkBody(model, { emu = true, tools = false, stream = false } = {}) {
  const body = { model, max_tokens: 64, stream, messages: [{ role: 'user', content: 'hi' }] };
  if (emu) {
    body.system = [{ type: 'text', text: identity }, { type: 'text', text: '请回复 pong' }];
    body.metadata = { user_id: JSON.stringify({ device_id: deviceId, account_uuid: '', session_id: sessionId }) };
  }
  if (tools) {
    body.tools = [
      { name: 'Bash', description: '执行 shell 命令', input_schema: { type: 'object', properties: { command: { type: 'string' } }, required: ['command'] } },
      { name: 'Read', description: '读文件', input_schema: { type: 'object', properties: { path: { type: 'string' } }, required: ['path'] } },
    ];
  }
  return body;
}
async function attempt(label, path, model, { beta = null, tools = false, emu = true, stream = false } = {}) {
  const h = {
    'content-type': 'application/json',
    'user-agent': 'claude-cli/2.1.252 (external, sdk-cli)',
    'x-app': 'cli',
    'anthropic-version': '2023-06-01',
    'anthropic-dangerous-direct-browser-access': 'true',
    authorization: `Bearer ${token}`,
  };
  const betas = ['claude-code-20250219'];
  if (beta) betas.push(beta);
  h['anthropic-beta'] = betas.join(',');
  const t0 = Date.now();
  const res = await fetch(base + path, { method: 'POST', headers: h, body: JSON.stringify(mkBody(model, { emu, tools, stream })) });
  const text = await res.text();
  console.log(`[${label}] HTTP ${res.status} (${Date.now() - t0}ms) ${res.status !== 200 ? text.slice(0, 100).replace(/\s+/g, ' ') : '✅ ' + text.slice(8, 60)}`);
}

const opus47 = (env.ANTHROPIC_DEFAULT_SONNET_MODEL ?? 'claude-opus-4-7[1M]').replace(/\[[^\]]+\]/g, '');
console.log('\nopus-4-7 变体:');
await attempt('opus47 仿真 无beta       ', '/v1/messages', opus47, {});
await attempt('opus47 仿真 +1m beta     ', '/v1/messages', opus47, { beta: 'context-1m-2025-08-07' });
await attempt('opus47 仿真 +1m+tools    ', '/v1/messages', opus47, { beta: 'context-1m-2025-08-07', tools: true });
console.log('fable 复测:');
const fable = (env.ANTHROPIC_DEFAULT_FABLE_MODEL ?? '').replace(/\[[^\]]+\]/g, '');
await attempt('fable 仿真 +1m（无tools） ', '/v1/messages', fable, { beta: 'context-1m-2025-08-07' });
await attempt('fable 仿真 +1m+tools     ', '/v1/messages', fable, { beta: 'context-1m-2025-08-07', tools: true });
db.close();
