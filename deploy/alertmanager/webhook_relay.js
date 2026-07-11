#!/usr/bin/env node
// ============================================================
//  Webhook 中继 — Alertmanager → 企业微信 / 钉钉
//  监听 9095 端口, 接收 Alertmanager webhook, 转发到群机器人
// ============================================================
const http = require('http');
const https = require('https');
const url = require('url');

const PORT = 9095;

// 从环境变量读取 Webhook URL
const WECOM_WEBHOOK = process.env.WECOM_WEBHOOK_URL || '';
const DINGTALK_WEBHOOK = process.env.DINGTALK_WEBHOOK_URL || '';
const DINGTALK_SECRET = process.env.DINGTALK_SECRET || '';

// ─── 钉钉加签 ───
function dingtalkSign(secret) {
  const crypto = require('crypto');
  const timestamp = Date.now();
  const stringToSign = `${timestamp}\n${secret}`;
  const hmac = crypto.createHmac('sha256', secret).update(stringToSign).digest('base64');
  return `&timestamp=${timestamp}&sign=${encodeURIComponent(hmac)}`;
}

// ─── 格式化告警为企微消息 ───
function formatWecom(alerts) {
  const status = alerts.every(a => a.status === 'resolved') ? '✅ 已恢复' : '🔴 告警';
  const lines = [`**【MMO网关${status}】**`, ''];

  alerts.forEach(a => {
    const icon = a.status === 'resolved' ? '✅' : (a.labels.severity === 'critical' ? '🔴' : '🟡');
    lines.push(`${icon} **${a.labels.alertname}**`);
    lines.push(`> 级别: ${a.labels.severity || 'unknown'}`);
    lines.push(`> 节点: ${a.labels.instance || a.labels.node || '-'}`);
    if (a.annotations.summary) lines.push(`> 摘要: ${a.annotations.summary}`);
    if (a.annotations.description) lines.push(`> 详情: ${a.annotations.description}`);
    lines.push(`> 开始: ${new Date(a.startsAt).toLocaleString('zh-CN')}`);
    if (a.endsAt && a.status === 'resolved') lines.push(`> 恢复: ${new Date(a.endsAt).toLocaleString('zh-CN')}`);
    lines.push('');
  });

  return {
    msgtype: 'markdown',
    markdown: { content: lines.join('\n') }
  };
}

// ─── 格式化告警为钉钉消息 ───
function formatDingtalk(alerts) {
  const isResolved = alerts.every(a => a.status === 'resolved');
  const title = isResolved ? '【MMO网关告警恢复】' : '【MMO网关告警】';
  const lines = [`### ${title}`, ''];

  alerts.forEach(a => {
    const icon = a.status === 'resolved' ? '✅' : (a.labels.severity === 'critical' ? '🔴' : '🟡');
    lines.push(`#### ${icon} ${a.labels.alertname}`);
    lines.push(`- **级别**: ${a.labels.severity || 'unknown'}`);
    lines.push(`- **节点**: ${a.labels.instance || a.labels.node || '-'}`);
    if (a.annotations.summary) lines.push(`- **摘要**: ${a.annotations.summary}`);
    if (a.annotations.description) lines.push(`- **详情**: ${a.annotations.description}`);
    lines.push(`- **时间**: ${new Date(a.startsAt).toLocaleString('zh-CN')}`);
    lines.push('');
  });

  return {
    msgtype: 'markdown',
    markdown: { title: title, text: lines.join('\n') }
  };
}

// ─── 发送 HTTP POST ───
function sendPost(targetUrl, body) {
  return new Promise((resolve, reject) => {
    const u = new URL(targetUrl);
    const data = JSON.stringify(body);
    const req = (u.protocol === 'https:' ? https : http).request({
      hostname: u.hostname,
      port: u.port || (u.protocol === 'https:' ? 443 : 80),
      path: u.pathname + u.search,
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(data) }
    }, res => {
      let d = ''; res.on('data', c => d += c); res.on('end', () => resolve({ status: res.statusCode, body: d }));
    });
    req.on('error', reject);
    req.write(data);
    req.end();
  });
}

// ─── HTTP 服务 ───
const server = http.createServer(async (req, res) => {
  const u = new URL(req.url, `http://localhost:${PORT}`);
  const path = u.pathname;
  const level = u.searchParams.get('level');

  if (req.method !== 'POST') {
    res.writeHead(200, { 'Content-Type': 'text/plain' });
    res.end('MMO Gateway Webhook Relay — OK');
    return;
  }

  let body = '';
  req.on('data', c => body += c);
  req.on('end', async () => {
    try {
      const payload = JSON.parse(body);
      const alerts = payload.alerts || [];

      if (path === '/wecom' && WECOM_WEBHOOK) {
        const msg = formatWecom(alerts);
        const result = await sendPost(WECOM_WEBHOOK, msg);
        console.log(`[wecom] ${alerts.length} alerts → ${result.status}`);
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ ok: true, sent: alerts.length, upstream: result.status }));
      } else if (path === '/dingtalk' && DINGTALK_WEBHOOK) {
        const msg = formatDingtalk(alerts);
        let targetUrl = DINGTALK_WEBHOOK;
        if (DINGTALK_SECRET) targetUrl += dingtalkSign(DINGTALK_SECRET);
        const result = await sendPost(targetUrl, msg);
        console.log(`[dingtalk] ${alerts.length} alerts → ${result.status}`);
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ ok: true, sent: alerts.length, upstream: result.status }));
      } else {
        console.log(`[relay] No webhook configured for ${path}`);
        res.writeHead(404);
        res.end(JSON.stringify({ ok: false, error: 'no webhook configured' }));
      }
    } catch (e) {
      console.error('[relay] Error:', e.message);
      res.writeHead(500);
      res.end(JSON.stringify({ ok: false, error: e.message }));
    }
  });
});

server.listen(PORT, '0.0.0.0', () => {
  console.log(`Webhook relay listening on :${PORT}`);
  console.log(`  企业微信: ${WECOM_WEBHOOK ? '✅ configured' : '❌ not set (WECOM_WEBHOOK_URL)'}`);
  console.log(`  钉钉: ${DINGTALK_WEBHOOK ? '✅ configured' : '❌ not set (DINGTALK_WEBHOOK_URL)'}`);
});
