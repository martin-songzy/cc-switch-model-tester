// 回显服务器：记录 exe 发来的真实请求（headers/body）到 /tmp/echo.log
import http from 'node:http';
import fs from 'node:fs';

const LOG = '/tmp/echo.log';
fs.writeFileSync(LOG, `=== 回显服务器启动 ${new Date().toISOString()} ===\n`);

http.createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    const lines = [
      `\n===== ${new Date().toISOString()} ${req.method} ${req.url} =====`,
      ...Object.entries(req.headers).map(([k, v]) => `${k}: ${k.toLowerCase().includes('key') || k.toLowerCase().includes('authorization') ? String(v).slice(0, 12) + '***' : v}`),
      `--- body ---`,
      body.slice(0, 2000),
    ];
    fs.appendFileSync(LOG, lines.join('\n') + '\n');
    console.log(lines.join('\n'));
    // 返回一个合法的 anthropic 非流式响应，让测试正常完成
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({
      id: 'msg_echo', type: 'message', role: 'assistant',
      content: [{ type: 'text', text: 'pong（回显）' }],
      model: 'echo', stop_reason: 'end_turn', usage: { input_tokens: 1, output_tokens: 1 },
    }));
  });
}).listen(9977, '127.0.0.1', () => console.log('echo server on 127.0.0.1:9977'));
