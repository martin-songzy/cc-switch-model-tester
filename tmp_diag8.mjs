// 临时诊断 8：system 元素数 + Authorization 大小写 二分定位 503 边界（不入仓库）
import { DatabaseSync } from 'node:sqlite';
import { createHash, randomUUID } from 'node:crypto';
import { hostname, homedir } from 'node:os';
import { join } from 'node:path';

const db = new DatabaseSync(join(homedir(), '.cc-switch', 'cc-switch.db'), { readOnly: true });
const sc = JSON.parse(db.prepare("SELECT settings_config FROM providers WHERE app_type='pi' AND id='anyrouter'").get().settings_config);
const url = sc.baseUrl.replace(/\/+$/, '') + '/v1/messages';
const deviceId = createHash('sha256').update('x:' + hostname()).digest('hex');
const sid = randomUUID();
const ccUA = 'claude-cli/2.1.252 (external, sdk-cli)';
const identity = 'You are a Claude agent, built on Anthropic' + String.fromCharCode(39) + 's Claude Agent SDK.';
const mk = (model, sysElems) => ({
  model, max_tokens: 64, stream: false,
  system: sysElems,
  metadata: { user_id: JSON.stringify({ device_id: deviceId, account_uuid: '', session_id: sid }) },
  messages: [{ role: 'user', content: 'hi' }],
});
async function go(label, sysElems, authKey) {
  const t0 = Date.now();
  try {
    const res = await fetch(url, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'user-agent': ccUA,
        'x-app': 'cli',
        'anthropic-version': '2023-06-01',
        'anthropic-dangerous-direct-browser-access': 'true',
        'anthropic-beta': 'claude-code-20250219,context-1m-2025-08-07',
        [authKey]: 'Bearer ' + sc.apiKey,
      },
      body: JSON.stringify(mk('claude-opus-4-7', sysElems)),
      signal: AbortSignal.timeout(15000),
    });
    const text = await res.text();
    console.log(`[${label}] HTTP ${res.status} (${Date.now() - t0}ms) ${res.status !== 200 ? text.slice(0, 70).replace(/\s+/g, ' ') : 'OK ' + text.slice(8, 40)}`);
  } catch (e) {
    console.log(`[${label}] ERR ${e.message.slice(0, 50)}`);
  }
}
await go('1元素system + 大写Auth', [{ type: 'text', text: identity }], 'Authorization');
await go('2元素system + 大写Auth', [{ type: 'text', text: identity }, { type: 'text', text: '请回复 pong' }], 'Authorization');
await go('1元素system + 小写auth', [{ type: 'text', text: identity }], 'authorization');
db.close();
