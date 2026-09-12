// 临时诊断 2：认证头类型 × 仿真 组合矩阵（不入仓库）
import { DatabaseSync } from 'node:sqlite';
import { createHash, randomUUID } from 'node:crypto';
import { hostname } from 'node:os';
import { homedir } from 'node:os';
import { join } from 'node:path';

const db = new DatabaseSync(join(homedir(), '.cc-switch', 'cc-switch.db'), { readOnly: true });
const rows = db.prepare("SELECT id, name, app_type, settings_config FROM providers").all();
const claude = rows.find((r) => r.app_type === 'claude' && (r.settings_config ?? '').includes('zzzcoding'));
const pi = rows.find((r) => r.app_type === 'pi' && (r.name.includes('seekai-copy') || r.settings_config.includes('zzzcoding-claudecode')));
const cEnv = JSON.parse(claude.settings_config).env;
const pSc = JSON.parse(pi.settings_config);
const baseUrl = 'https://api.zzzcoding.org';
const bearerToken = cEnv.ANTHROPIC_AUTH_TOKEN;
const apiKey = pSc.apiKey;
console.log(`claude AUTH_TOKEN 前缀: ${bearerToken.slice(0, 10)}… | pi apiKey 前缀: ${apiKey.slice(0, 10)}… | 同 key: ${bearerToken === apiKey}`);

const deviceId = createHash('sha256').update(`pi-client-emulation:${hostname()}`).digest('hex');
const sessionId = randomUUID();
const identity = 'You are a Claude agent, built on Anthropic' + String.fromCharCode(39) + 's Claude Agent SDK.';

function body(withEmu = true) {
  const b = {
    model: 'claude-opus-5[1M]',
    max_tokens: 64,
    stream: false,
    messages: [{ role: 'user', content: 'hi' }],
  };
  if (withEmu) {
    b.system = [{ type: 'text', text: identity }, { type: 'text', text: '请回复 pong' }];
    b.metadata = { user_id: JSON.stringify({ device_id: deviceId, account_uuid: '', session_id: sessionId }) };
  }
  return b;
}
const ccHeaders = {
  'user-agent': 'claude-cli/2.1.252 (external, sdk-cli)',
  'x-app': 'cli',
  'anthropic-beta': 'claude-code-20250219',
  'anthropic-version': '2023-06-01',
  'anthropic-dangerous-direct-browser-access': 'true',
};

async function attempt(label, headers) {
  const t0 = Date.now();
  const res = await fetch(baseUrl + '/v1/messages', {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify(body(true)),
  });
  const text = await res.text();
  console.log(`[${label}] HTTP ${res.status} (${Date.now() - t0}ms) ${text.slice(0, 120).replace(/\s+/g, ' ')}`);
}

await attempt('A Bearer+仿真(工具行为)', { authorization: `Bearer ${bearerToken}`, ...ccHeaders });
await attempt('B x-api-key+仿真', { 'x-api-key': apiKey, ...ccHeaders });
await attempt('C x-api-key 无仿真', { 'x-api-key': apiKey });
await attempt('D Bearer 无仿真', { authorization: `Bearer ${bearerToken}` });
await attempt('E Bearer+仿真+oauth-beta', { authorization: `Bearer ${bearerToken}`, ...ccHeaders, 'anthropic-beta': 'oauth-2025-04-20,claude-code-20250219' });
await attempt('F Bearer+仿真+x-api-key 同发', { authorization: `Bearer ${bearerToken}`, 'x-api-key': apiKey, ...ccHeaders });
db.close();
