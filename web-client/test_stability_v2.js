/**
 * Stability Stress Test v2 - Rust MMO Gateway
 *
 * 优化: 全局消息定时器 + 异步内存采集 + 自动重连
 * 支持 2500+ 并发连接长时间稳定运行
 *
 * Usage:
 *   node test_stability_v2.js --connections 2500 --duration 30
 *   node test_stability_v2.js --connections 5000 --duration 1440
 */

const net = require("net");
const crypto = require("crypto");
const fs = require("fs");
const path = require("path");
const http = require("http");
const { exec } = require("child_process");

// ── 配置 ────────────────────────────────────────────────────────────
const args = process.argv.slice(2);
function getArg(name, def) {
  const idx = args.indexOf(`--${name}`);
  if (idx >= 0 && args[idx + 1]) return args[idx + 1];
  return def;
}

const CONFIG = {
  host: getArg("host", "127.0.0.1"),
  tcpPort: parseInt(getArg("tcpPort", "7888")),
  httpPort: parseInt(getArg("httpPort", "9090")),
  connections: parseInt(getArg("connections", "2500")),
  durationMin: parseInt(getArg("duration", "30")),
  connectBatchSize: parseInt(getArg("batchSize", "100")),
  connectDelayMs: parseInt(getArg("delay", "100")),
  globalMsgIntervalMs: parseInt(getArg("msgInterval", "1000")), // 全局定时器间隔
  msgBatchSize: parseInt(getArg("msgBatch", "100")), // 每次定时器发送的消息数(需确保2500客户端在45s内心跳保活: 2500/45≈56, 设100保险)
  aesKey: getArg("aesKey", "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"),
  reportDir: path.join(__dirname, "stability_reports"),
  autoReconnect: getArg("reconnect", "true") !== "false",
};

const DURATION_SEC = CONFIG.durationMin * 60;
const SAMPLE_INTERVAL_SEC = 60;

// ── 消息 ID ─────────────────────────────────────────────────────────
const MSG = {
  HANDSHAKE: 0x0001, MOVE: 3001, CHAT: 2001, ATTACK: 1001, QUERY: 4001,
  STATS: 5001, CHAT_ACK: 7001, CHAT_BROADCAST: 7002,
  POSITION_UPDATE: 8001, PLAYER_ENTER: 8002, PLAYER_LEAVE: 8003,
  PLAYER_LIST: 9001, BATTLE: 6001,
};

// ── CRC32 + AES ────────────────────────────────────────────────────
const crc32Table = (() => {
  const t = new Uint32Array(256);
  for (let i = 0; i < 256; i++) { let c = i; for (let j = 0; j < 8; j++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[i] = c >>> 0; }
  return t;
})();
function crc32(buf) {
  let crc = 0xffffffff;
  for (let i = 0; i < buf.length; i++) crc = crc32Table[(crc ^ buf[i]) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

const aesKey = Buffer.from(CONFIG.aesKey, "hex");
function encrypt(plaintext) {
  const nonce = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv("aes-256-gcm", aesKey, nonce);
  const encrypted = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  const tag = cipher.getAuthTag();
  return Buffer.concat([nonce, encrypted, tag]);
}
function decrypt(data) {
  const nonce = data.subarray(0, 12);
  const tag = data.subarray(data.length - 16);
  const ciphertext = data.subarray(12, data.length - 16);
  const decipher = crypto.createDecipheriv("aes-256-gcm", aesKey, nonce);
  decipher.setAuthTag(tag);
  return Buffer.concat([decipher.update(ciphertext), decipher.final()]);
}

// ── 协议编解码 ──────────────────────────────────────────────────────
const HEADER_SIZE = 16;
const MAGIC = [0x4d, 0x4d];
const PROTOCOL_VERSION = 1;
const MAX_BODY_SIZE = 8192;

function encodePacket(msgId, plaintext) {
  const encrypted = encrypt(plaintext);
  const header = Buffer.alloc(HEADER_SIZE);
  header[0] = MAGIC[0]; header[1] = MAGIC[1];
  header[2] = PROTOCOL_VERSION; header[3] = 0;
  header.writeUInt16BE(msgId, 4);
  header.writeUInt16BE(encrypted.length, 6);
  header.writeUInt32BE(crc32(encrypted), 8);
  header.writeUInt32BE(0, 12);
  return Buffer.concat([header, encrypted]);
}

function encodeHandshake(uid, token) {
  return encodePacket(0x0001, Buffer.from(JSON.stringify({
    uid, token, version: PROTOCOL_VERSION, timestamp: Math.floor(Date.now() / 1000),
  }), "utf8"));
}

// ── 全局统计 ────────────────────────────────────────────────────────
const stats = {
  startTime: 0,
  connected: 0,
  disconnected: 0,
  reconnectAttempts: 0,
  reconnectSuccess: 0,
  totalSent: 0,
  totalReceived: 0,
  totalErrors: 0,
  sentByType: {},
  receivedByType: {},
  latencies: [],
  samples: [],
  crashDetected: false,
  crashTime: null,
};

const allClients = [];
let gatewayPid = null;
let logStream = null;

// ── 异步 exec ──────────────────────────────────────────────────────
function execAsync(cmd, timeoutMs = 5000) {
  return new Promise((resolve) => {
    exec(cmd, { timeout: timeoutMs }, (err, stdout) => {
      resolve(err ? "" : stdout);
    });
  });
}

async function findGatewayPid() {
  const output = await execAsync(`netstat -ano | findstr ":${CONFIG.tcpPort}.*LISTENING"`);
  const match = output.match(/(\d+)\s*$/m);
  if (match) {
    gatewayPid = parseInt(match[1]);
    return gatewayPid;
  }
  return null;
}

async function getProcessMemMB(pid) {
  const output = await execAsync(
    `powershell -NoProfile -Command "(Get-Process -Id ${pid}).WorkingSet64"`,
    8000
  );
  const memBytes = parseInt(output.trim());
  return memBytes > 0 ? memBytes / 1024 / 1024 : 0;
}

// ── HTTP 健康检查 ───────────────────────────────────────────────────
function httpGet(port, urlPath) {
  return new Promise((resolve) => {
    const req = http.get({ hostname: CONFIG.host, port, path: urlPath, timeout: 5000 }, (res) => {
      let data = "";
      res.on("data", (c) => (data += c));
      res.on("end", () => {
        try { resolve({ status: res.statusCode, body: JSON.parse(data) }); }
        catch { resolve({ status: res.statusCode, body: data }); }
      });
    });
    req.on("error", () => resolve({ status: 0, error: true }));
    req.on("timeout", () => { req.destroy(); resolve({ status: 0, error: true }); });
  });
}

// ── 客户端 ──────────────────────────────────────────────────────────
class StressClient {
  constructor(uid) {
    this.uid = uid;
    this.socket = null;
    this.decoder = null;
    this.connected = false;
    this.msgsSent = 0;
    this.msgsReceived = 0;
    this.connectTime = 0;
    this.pendingMoveTs = [];
  }

  connect() {
    return new Promise((resolve, reject) => {
      this.socket = new net.Socket();
      this.socket.setNoDelay(true);
      // Increase buffer sizes for better throughput
      this.socket.setDefaultEncoding("utf8");

      const timeout = setTimeout(() => {
        this.socket.destroy();
        reject(new Error("connect timeout"));
      }, 10000);

      this.socket.connect(CONFIG.tcpPort, CONFIG.host, () => {
        clearTimeout(timeout);
        this.connected = true;
        this.connectTime = Date.now();
        this.decoder = { buffer: Buffer.alloc(0) };

        // 发送握手
        const hs = encodeHandshake(this.uid, "test_token_123");
        this.socket.write(hs);
        this.msgsSent++;
        stats.totalSent++;

        resolve();
      });

      this.socket.on("data", (data) => {
        if (!this.decoder) return;
        this.decoder.buffer = Buffer.concat([this.decoder.buffer, data]);

        while (this.decoder.buffer.length >= HEADER_SIZE) {
          try {
            if (this.decoder.buffer[0] !== MAGIC[0] || this.decoder.buffer[1] !== MAGIC[1]) {
              throw new Error("magic mismatch");
            }
            const msgId = this.decoder.buffer.readUInt16BE(4);
            const bodyLen = this.decoder.buffer.readUInt16BE(6);
            const crc = this.decoder.buffer.readUInt32BE(8);
            if (bodyLen > MAX_BODY_SIZE) throw new Error("body too large");
            const totalLen = HEADER_SIZE + bodyLen;
            if (this.decoder.buffer.length < totalLen) break;

            const body = this.decoder.buffer.subarray(HEADER_SIZE, totalLen);
            if (crc32(body) !== crc) throw new Error("CRC mismatch");

            const plaintext = decrypt(body);
            this.msgsReceived++;
            stats.totalReceived++;
            stats.receivedByType[msgId] = (stats.receivedByType[msgId] || 0) + 1;

            // Track move latency
            if (msgId === MSG.POSITION_UPDATE) {
              try {
                const d = JSON.parse(plaintext.toString("utf8"));
                if (d.uid === this.uid && this.pendingMoveTs.length > 0) {
                  const ts = this.pendingMoveTs.shift();
                  const lat = Date.now() - ts;
                  if (lat > 0 && lat < 10000) {
                    stats.latencies.push(lat);
                    if (stats.latencies.length > 5000) stats.latencies.shift();
                  }
                }
              } catch {}
            }

            this.decoder.buffer = this.decoder.buffer.subarray(totalLen);
          } catch (e) {
            stats.totalErrors++;
            // Don't disconnect on decode error - just reset buffer
            this.decoder.buffer = Buffer.alloc(0);
            break;
          }
        }
      });

      this.socket.on("error", (err) => {
        clearTimeout(timeout);
        if (!this.connected) reject(err);
      });

      this.socket.on("close", () => {
        clearTimeout(timeout);
        if (this.connected) {
          this.connected = false;
          stats.connected--;
          stats.disconnected++;
        }
      });
    });
  }

  sendMessage() {
    if (!this.connected || !this.socket) return;
    const rand = Math.random();
    let msgId, data;

    // 消息分布: 80% QUERY(不触发广播), 10% MOVE, 5% CHAT, 5% ATTACK
    if (rand < 0.80) {
      msgId = MSG.QUERY;
      data = JSON.stringify({});
    } else if (rand < 0.90) {
      msgId = MSG.MOVE;
      data = JSON.stringify({
        x: Math.floor(Math.random() * 1600),
        y: Math.floor(Math.random() * 1200),
        dir: Math.floor(Math.random() * 4),
      });
      this.pendingMoveTs.push(Date.now());
      if (this.pendingMoveTs.length > 5) this.pendingMoveTs.shift();
    } else if (rand < 0.95) {
      msgId = MSG.CHAT;
      data = JSON.stringify({ text: `stress-${this.uid}-${Date.now()}` });
    } else {
      msgId = MSG.ATTACK;
      data = JSON.stringify({ targetUid: 20000 + Math.floor(Math.random() * 16) });
    }

    try {
      const pkt = encodePacket(msgId, Buffer.from(data, "utf8"));
      this.socket.write(pkt);
      this.msgsSent++;
      stats.totalSent++;
      stats.sentByType[msgId] = (stats.sentByType[msgId] || 0) + 1;
    } catch {
      stats.totalErrors++;
    }
  }

  disconnect() {
    if (this.socket) {
      this.connected = false;
      this.socket.destroy();
      this.socket = null;
    }
  }

  async reconnect() {
    if (this.connected) return true;
    stats.reconnectAttempts++;
    try {
      await this.connect();
      stats.reconnectSuccess++;
      return true;
    } catch {
      return false;
    }
  }
}

// ── 日志 ────────────────────────────────────────────────────────────
function log(msg) {
  const ts = new Date().toISOString();
  const line = `[${ts}] ${msg}`;
  console.log(line);
  if (logStream) logStream.write(line + "\n");
}

// ── 采样 ────────────────────────────────────────────────────────────
let lastSampleSent = 0;
let lastSampleReceived = 0;
let lastSampleTime = 0;

async function sample(elapsedSec) {
  const health = await httpGet(CONFIG.httpPort, "/health");
  let memMB = 0;
  if (gatewayPid) {
    memMB = await getProcessMemMB(gatewayPid);
  }

  const lats = stats.latencies;
  let latP50 = 0, latP99 = 0, latAvg = 0;
  if (lats.length > 0) {
    const sorted = [...lats].sort((a, b) => a - b);
    latP50 = sorted[Math.floor(sorted.length * 0.5)];
    latP99 = sorted[Math.floor(sorted.length * 0.99)];
    latAvg = sorted.reduce((a, b) => a + b, 0) / sorted.length;
  }

  const now = Date.now();
  const intervalSec = (now - lastSampleTime) / 1000;
  const msgRate = intervalSec > 0 ? Math.round((stats.totalSent - lastSampleSent) / intervalSec) : 0;
  const recvRate = intervalSec > 0 ? Math.round((stats.totalReceived - lastSampleReceived) / intervalSec) : 0;

  const s = {
    elapsedSec, timestamp: new Date().toISOString(),
    connected: stats.connected, target: CONFIG.connections,
    gatewayOnline: health.status === 200,
    gatewayOnlineCount: health.body?.online_count || 0,
    msgRate, recvRate,
    totalSent: stats.totalSent, totalReceived: stats.totalReceived,
    totalErrors: stats.totalErrors, disconnects: stats.disconnected,
    reconnectAttempts: stats.reconnectAttempts,
    latP50, latP99, latAvg: latAvg.toFixed(1), latCount: lats.length,
    memMB: memMB.toFixed(2),
  };
  stats.samples.push(s);

  lastSampleSent = stats.totalSent;
  lastSampleReceived = stats.totalReceived;
  lastSampleTime = now;

  log(
    `[${Math.floor(elapsedSec/60)}m${String(elapsedSec%60).padStart(2,"0")}s] ` +
    `Conn: ${s.connected}/${s.target} | ` +
    `GW: ${s.gatewayOnline ? "UP" : "DOWN"} (${s.gatewayOnlineCount}) | ` +
    `Msg: ${s.msgRate}/s out, ${s.recvRate}/s in | ` +
    `Total: ${s.totalSent} sent, ${s.totalReceived} recv | ` +
    `Err: ${s.totalErrors} | Disc: ${s.disconnects} | ` +
    `Reconn: ${s.reconnectAttempts} | ` +
    `Lat: P50=${s.latP50}ms P99=${s.latP99}ms (${s.latCount}) | ` +
    `Mem: ${s.memMB}MB`
  );

  if (!s.gatewayOnline && !stats.crashDetected) {
    stats.crashDetected = true;
    stats.crashTime = new Date().toISOString();
    log(`!!! CRASH DETECTED at ${stats.crashTime} !!!`);
  }
}

// ── HTML 报告生成 ──────────────────────────────────────────────────
function generateHTMLReport() {
  const endTime = Date.now();
  const totalSec = Math.floor((endTime - stats.startTime) / 1000);
  const memData = stats.samples.filter(s => parseFloat(s.memMB) > 0).map(s => ({ x: s.elapsedSec, y: parseFloat(s.memMB) }));
  const lastMem = memData.length > 0 ? memData[memData.length-1].y : 0;
  const firstMem = memData.length > 0 ? memData[0].y : 0;
  const memGrowth = lastMem - firstMem;
  const memGrowthPct = firstMem > 0 ? ((memGrowth / firstMem) * 100).toFixed(2) : 0;
  const noCrash = !stats.crashDetected;
  const noMemoryLeak = parseFloat(memGrowthPct) < 20;
  const msgLossRate = stats.totalSent > 0 ? ((1 - stats.totalReceived / Math.max(stats.totalSent, 1)) * 100).toFixed(2) : 0;
  const passed = noCrash && noMemoryLeak;
  const avgMsgRate = stats.samples.length > 0 ? Math.round(stats.samples.reduce((a,s) => a + s.msgRate, 0) / stats.samples.length) : 0;
  const peakMsgRate = stats.samples.length > 0 ? Math.max(...stats.samples.map(s => s.msgRate)) : 0;

  return `<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Stability Test Report</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#f5f5f5;color:#333}
.container{max-width:1200px;margin:0 auto;padding:20px}
.header{background:linear-gradient(135deg,#1a1a2e,#16213e);color:#fff;padding:30px;border-radius:12px;margin-bottom:24px}
.header h1{font-size:28px;margin-bottom:8px}
.badge{display:inline-block;padding:6px 16px;border-radius:20px;font-weight:bold;font-size:14px;margin-top:12px}
.badge-pass{background:#4caf50;color:#fff}.badge-fail{background:#f44336;color:#fff}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:16px;margin-bottom:24px}
.card{background:#fff;padding:20px;border-radius:10px;box-shadow:0 2px 8px rgba(0,0,0,.08)}
.card .label{font-size:12px;color:#888;text-transform:uppercase;margin-bottom:8px}
.card .value{font-size:28px;font-weight:700;color:#1a1a2e}
.card .unit{font-size:14px;font-weight:400;color:#888}
.card .detail{font-size:12px;color:#666;margin-top:4px}
.section{background:#fff;padding:24px;border-radius:10px;box-shadow:0 2px 8px rgba(0,0,0,.08);margin-bottom:24px}
.section h2{font-size:18px;margin-bottom:16px;color:#1a1a2e;border-bottom:2px solid #e0e0e0;padding-bottom:8px}
table{width:100%;border-collapse:collapse;font-size:13px}
th,td{padding:8px 12px;text-align:left;border-bottom:1px solid #eee}
th{background:#f8f8f8;font-weight:600;color:#555}
.check-item{display:flex;align-items:center;padding:12px 0;border-bottom:1px solid #eee}
.check-icon{width:24px;height:24px;border-radius:50%;display:flex;align-items:center;justify-content:center;margin-right:12px;font-weight:bold;color:#fff}
.check-pass{background:#4caf50}.check-fail{background:#f44336}
.footer{text-align:center;padding:20px;color:#888;font-size:12px}
</style>
</head>
<body><div class="container">
<div class="header">
<h1>Rust MMO Gateway - Stability Test Report</h1>
<div style="font-size:14px;opacity:.8">Duration: ${totalSec}s | Target: ${CONFIG.connections} connections | Start: ${new Date(stats.startTime).toISOString()}</div>
<div class="badge ${passed?"badge-pass":"badge-fail"}">${passed?"PASS":"FAIL"}</div>
</div>
<div class="grid">
<div class="card"><div class="label">Final Connections</div><div class="value">${stats.connected}<span class="unit">/${CONFIG.connections}</span></div><div class="detail">Disconnected: ${stats.disconnected} | Reconnected: ${stats.reconnectSuccess}</div></div>
<div class="card"><div class="label">Messages Sent</div><div class="value">${stats.totalSent.toLocaleString()}</div><div class="detail">Errors: ${stats.totalErrors}</div></div>
<div class="card"><div class="label">Messages Received</div><div class="value">${stats.totalReceived.toLocaleString()}</div><div class="detail">Loss rate: ${msgLossRate}%</div></div>
<div class="card"><div class="label">Avg Msg Rate</div><div class="value">${avgMsgRate}<span class="unit">/s</span></div><div class="detail">Peak: ${peakMsgRate}/s</div></div>
<div class="card"><div class="label">Latency P99</div><div class="value">${stats.samples.filter(s=>s.latCount>0).length>0?Math.max(...stats.samples.filter(s=>s.latCount>0).map(s=>s.latP99)):0}<span class="unit">ms</span></div><div class="detail">P50 avg: ${stats.samples.filter(s=>s.latCount>0).length>0?Math.round(stats.samples.filter(s=>s.latCount>0).reduce((a,s)=>a+s.latP50,0)/stats.samples.filter(s=>s.latCount>0).length):0}ms</div></div>
<div class="card"><div class="label">Memory (Final)</div><div class="value">${lastMem.toFixed(2)}<span class="unit">MB</span></div><div class="detail">Growth: ${memGrowth>=0?"+":""}${memGrowth.toFixed(2)}MB (${memGrowthPct}%)</div></div>
</div>
<div class="section"><h2>Verification Checklist</h2>
<div class="check-item"><div class="check-icon ${noCrash?"check-pass":"check-fail"}">${noCrash?"&#10003;":"&#10007;"}</div><div><div style="font-weight:600">No Crash / Panic</div><div style="font-size:12px;color:#888">${noCrash?"Gateway stable throughout":"Crash at "+(stats.crashTime||"unknown")}</div></div></div>
<div class="check-item"><div class="check-icon ${noMemoryLeak?"check-pass":"check-fail"}">${noMemoryLeak?"&#10003;":"&#10007;"}</div><div><div style="font-weight:600">No Memory Leak</div><div style="font-size:12px;color:#888">Growth: ${memGrowthPct}% (${memGrowth>=0?"+":""}${memGrowth.toFixed(2)}MB)</div></div></div>
<div class="check-item"><div class="check-icon ${stats.disconnected===0?"check-pass":"check-fail"}">${stats.disconnected===0?"&#10003;":"&#10007;"}</div><div><div style="font-weight:600">No Unexpected Disconnects</div><div style="font-size:12px;color:#888">${stats.disconnected===0?"All connections maintained":stats.disconnected+" disconnects, "+stats.reconnectSuccess+" reconnected"}</div></div></div>
<div class="check-item"><div class="check-icon ${parseFloat(msgLossRate)<5?"check-pass":"check-fail"}">${parseFloat(msgLossRate)<5?"&#10003;":"&#10007;"}</div><div><div style="font-weight:600">Low Message Loss</div><div style="font-size:12px;color:#888">Rate: ${msgLossRate}%</div></div></div>
</div>
<div class="section"><h2>Monitoring Data</h2>
<table><tr><th>Time</th><th>Connected</th><th>Gateway</th><th>Msg Out/s</th><th>Msg In/s</th><th>Total Sent</th><th>Total Recv</th><th>Errors</th><th>Disc</th><th>Lat P50</th><th>Lat P99</th><th>Mem MB</th></tr>
${stats.samples.map(s=>`<tr><td>${Math.floor(s.elapsedSec/60)}m${s.elapsedSec%60}s</td><td>${s.connected}/${s.target}</td><td style="color:${s.gatewayOnline?"#4caf50":"#f44336"}">${s.gatewayOnline?"UP":"DOWN"}</td><td>${s.msgRate}</td><td>${s.recvRate}</td><td>${s.totalSent.toLocaleString()}</td><td>${s.totalReceived.toLocaleString()}</td><td>${s.totalErrors}</td><td>${s.disconnects}</td><td>${s.latP50}ms</td><td>${s.latP99}ms</td><td>${s.memMB}</td></tr>`).join("")}
</table></div>
<div class="section" style="background:linear-gradient(135deg,#1a1a2e,#16213e);color:#fff">
<h2 style="color:#fff;border-color:rgba(255,255,255,.2)">Conclusion</h2>
<p style="margin-top:12px;line-height:1.8">
Duration: <strong>${Math.floor(totalSec/3600)}h ${Math.floor((totalSec%3600)/60)}m ${totalSec%60}s</strong><br>
Connections: <strong>${stats.connected}/${CONFIG.connections}</strong><br>
Messages: <strong>${stats.totalSent.toLocaleString()} sent / ${stats.totalReceived.toLocaleString()} received</strong><br>
Loss Rate: <strong>${msgLossRate}%</strong><br>
Memory Growth: <strong>${memGrowthPct}%</strong><br>
Crash: <strong>${stats.crashDetected?"YES":"NO"}</strong><br><br>
Result: <strong style="font-size:18px">${passed?"PASS - All checks passed":"FAIL - See issues above"}</strong>
</p></div>
<div class="footer">Generated: ${new Date().toISOString()}</div>
</div></body></html>`;
}

// ── 主流程 ──────────────────────────────────────────────────────────
async function main() {
  console.log("\n======================================================");
  console.log("  Rust MMO Gateway - Stability Stress Test v2");
  console.log("======================================================");
  console.log(`  Target:    ${CONFIG.connections} connections`);
  console.log(`  Duration:  ${CONFIG.durationMin} minutes`);
  console.log(`  Gateway:   ${CONFIG.host}:${CONFIG.tcpPort} (HTTP ${CONFIG.httpPort})`);
  console.log("======================================================\n");

  if (!fs.existsSync(CONFIG.reportDir)) fs.mkdirSync(CONFIG.reportDir, { recursive: true });
  const logFile = path.join(CONFIG.reportDir, `stability_${Date.now()}.log`);
  logStream = fs.createWriteStream(logFile, { flags: "w" });
  stats.startTime = Date.now();
  lastSampleTime = Date.now();
  log(`Stability test v2 started. Log: ${logFile}`);

  // 检查网关
  const health = await httpGet(CONFIG.httpPort, "/health");
  if (health.status !== 200) {
    log(`ERROR: Gateway not reachable. Start it first.`);
    process.exit(1);
  }
  log(`Gateway online. Online: ${health.body?.online_count || 0}`);

  // 查找 PID
  const pid = await findGatewayPid();
  if (pid) {
    log(`Gateway PID: ${pid}`);
  }

  // 建立连接
  log(`Phase 1: Establishing ${CONFIG.connections} connections (batch=${CONFIG.connectBatchSize}, delay=${CONFIG.connectDelayMs}ms)...`);
  let success = 0, fail = 0;
  const connStart = Date.now();

  for (let bs = 0; bs < CONFIG.connections; bs += CONFIG.connectBatchSize) {
    const be = Math.min(bs + CONFIG.connectBatchSize, CONFIG.connections);
    const batch = [];
    for (let i = bs; i < be; i++) {
      const uid = 50000 + i;
      const client = new StressClient(uid);
      allClients.push(client);
      batch.push(client.connect().then(() => { success++; stats.connected++; }, () => { fail++; }));
    }
    await Promise.all(batch);
    if (be < CONFIG.connections) {
      const pct = ((be / CONFIG.connections) * 100).toFixed(1);
      log(`  Progress: ${be}/${CONFIG.connections} (${pct}%) - OK: ${success}, Fail: ${fail}`);
      await new Promise(r => setTimeout(r, CONFIG.connectDelayMs));
    }
  }
  const connDur = ((Date.now() - connStart) / 1000).toFixed(1);
  log(`Phase 1 complete: ${success} connected, ${fail} failed in ${connDur}s (${Math.round(success / parseFloat(connDur))} conn/s)`);

  // ── Phase 1.5: 合包压缩率验证（2分钟高频流量）──────────────────────
  // 向少量客户端(100)高频发送突发包(每50ms发5个)，模拟团战场景
  // 使每个客户端的 WriteLoop 在16ms窗口内收到多个包，触发合包
  const mergeVerifyClients = 100;
  const mergeVerifyDurationMs = 120000; // 2 minutes
  const mergeVerifyBurstSize = 5;       // 每次发5个包给同一客户端
  const mergeVerifyIntervalMs = 50;     // 每50ms发一次

  log(`Phase 1.5: Merge compression verification (${mergeVerifyClients} clients, ${mergeVerifyBurstSize} pkts/burst, ${mergeVerifyIntervalMs}ms interval, ${mergeVerifyDurationMs/1000}s)`);
  // 先调用 /merge_stats 触发快照基线
  await new Promise((resolve) => {
    const req = http.get("http://" + CONFIG.host + ":" + CONFIG.httpPort + "/merge_stats", (res) => {
      res.on("data", () => {});
      res.on("end", resolve);
    });
    req.on("error", resolve);
  });

  let mergeClientIdx = 0;
  const mergeVerifyTimer = setInterval(() => {
    for (let i = 0; i < mergeVerifyBurstSize; i++) {
      const client = allClients[mergeClientIdx % mergeVerifyClients];
      if (client && client.connected) client.sendMessage();
    }
    mergeClientIdx++;
  }, mergeVerifyIntervalMs);

  // 等待合包验证完成
  await new Promise(r => setTimeout(r, mergeVerifyDurationMs));
  clearInterval(mergeVerifyTimer);

  // 采集合包验证结果
  const mergeResult = await new Promise((resolve) => {
    http.get("http://" + CONFIG.host + ":" + CONFIG.httpPort + "/merge_stats", (res) => {
      let data = "";
      res.on("data", (chunk) => data += chunk);
      res.on("end", () => {
        try { resolve(JSON.parse(data)); } catch { resolve({}); }
      });
    }).on("error", () => resolve({}));
  });
  log(`Phase 1.5 result: recent_rate=${mergeResult.recent_compression_rate_pct || "N/A"}% recent_avg=${mergeResult.recent_avg_packets_per_flush || "N/A"} pkts/flush (target: >=70%)`);
  if (parseFloat(mergeResult.recent_compression_rate_pct || "0") >= 70) {
    log("  ✅ Merge compression gate PASSED (>=70%)");
  } else {
    log(`  ⚠️ Merge compression rate ${mergeResult.recent_compression_rate_pct || "N/A"}% (below 70% target)`);
  }

  // ── Phase 2: 长稳测试（低频心跳流量）──────────────────────────────
  // 全局消息定时器（替代每客户端定时器）
  log(`Phase 2: Running for ${CONFIG.durationMin} minutes (global msg interval: ${CONFIG.globalMsgIntervalMs}ms)`);
  let msgClientIdx = 0;
  const globalMsgTimer = setInterval(() => {
    // 每次定时器触发，发送一批消息(确保所有客户端在45s心跳超时内至少发一次)
    const batchSize = Math.min(CONFIG.msgBatchSize, allClients.length);
    for (let i = 0; i < batchSize; i++) {
      const client = allClients[msgClientIdx % allClients.length];
      msgClientIdx++;
      if (client.connected) client.sendMessage();
    }
  }, CONFIG.globalMsgIntervalMs);

  // 自动重连定时器
  const reconnectTimer = CONFIG.autoReconnect ? setInterval(() => {
    for (const client of allClients) {
      if (!client.connected) {
        client.reconnect().then((ok) => {
          if (ok) stats.connected++;
        }).catch(() => {});
      }
    }
  }, 10000) : null;

  // 采样定时器
  const sampleTimer = setInterval(async () => {
    const elapsed = Math.floor((Date.now() - stats.startTime) / 1000);
    await sample(elapsed);

    // 内存泄漏警告
    if (stats.samples.length >= 5) {
      const recent = stats.samples.slice(-5).filter(s => parseFloat(s.memMB) > 0);
      if (recent.length >= 3) {
        const mems = recent.map(s => parseFloat(s.memMB));
        if (mems.every((m, i) => i === 0 || m >= mems[i-1] - 0.5)) {
          const growth = mems[mems.length-1] - mems[0];
          if (growth > 5) log(`  WARNING: Memory increasing (+${growth.toFixed(2)}MB in 5 samples)`);
        }
      }
    }
  }, SAMPLE_INTERVAL_SEC * 1000);

  // 等待测试结束
  await new Promise(r => setTimeout(r, DURATION_SEC * 1000));

  // 清理
  clearInterval(globalMsgTimer);
  clearInterval(sampleTimer);
  if (reconnectTimer) clearInterval(reconnectTimer);

  // 最终采样
  const finalElapsed = Math.floor((Date.now() - stats.startTime) / 1000);
  await sample(finalElapsed);

  // 断开所有
  log("Disconnecting all clients...");
  for (const c of allClients) c.disconnect();

  // 报告
  log("Generating report...");
  const html = generateHTMLReport();
  const reportFile = path.join(CONFIG.reportDir, `stability_report_${Date.now()}.html`);
  fs.writeFileSync(reportFile, html);

  log("\n======================================================");
  log("  Stability Test Complete");
  log("======================================================");
  log(`  Duration:      ${finalElapsed}s (${Math.floor(finalElapsed/3600)}h ${Math.floor((finalElapsed%3600)/60)}m ${finalElapsed%60}s)`);
  log(`  Connected:     ${stats.connected}/${CONFIG.connections}`);
  log(`  Total Sent:    ${stats.totalSent.toLocaleString()}`);
  log(`  Total Recv:    ${stats.totalReceived.toLocaleString()}`);
  log(`  Errors:        ${stats.totalErrors}`);
  log(`  Disconnects:   ${stats.disconnected}`);
  log(`  Reconnects:    ${stats.reconnectSuccess}/${stats.reconnectAttempts}`);
  log(`  Crash:         ${stats.crashDetected}`);
  log(`  Memory:        ${stats.samples.length > 0 ? stats.samples[stats.samples.length-1].memMB : 0}MB`);
  log(`  Report:        ${reportFile}`);
  log("======================================================");

  if (logStream) logStream.end();
  process.exit(0);
}

main().catch((e) => {
  console.error("Fatal:", e);
  if (logStream) logStream.end();
  process.exit(1);
});
