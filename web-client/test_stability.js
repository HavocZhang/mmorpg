/**
 * Stability Stress Test - Rust MMO Gateway
 *
 * 持续模拟 2500 并发连接 + 高频消息收发
 * 每 60 秒采集: 内存/CPU/连接数/消息丢失率/延迟P99
 * 自动检测崩溃/内存泄漏/异常断连
 * 输出监控日志 + 最终 HTML 报告
 *
 * Usage:
 *   node test_stability.js --duration 30        // 30 分钟
 *   node test_stability.js --duration 1440      // 24 小时
 *   node test_stability.js --duration 4320      // 72 小时
 *   node test_stability.js --connections 2500 --duration 30
 */

const net = require("net");
const crypto = require("crypto");
const fs = require("fs");
const path = require("path");
const http = require("http");
const { execSync } = require("child_process");

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
  durationMin: parseInt(getArg("duration", "30")), // 分钟
  connectBatchSize: parseInt(getArg("batchSize", "200")),
  connectDelayMs: parseInt(getArg("delay", "50")),
  msgIntervalMs: parseInt(getArg("msgInterval", "500")), // 每个客户端消息间隔
  aesKey: getArg("aesKey", "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"),
  reportDir: path.join(__dirname, "stability_reports"),
};

const DURATION_SEC = CONFIG.durationMin * 60;
const SAMPLE_INTERVAL_SEC = 60; // 每 60 秒采样一次

// ── 消息 ID ─────────────────────────────────────────────────────────
const MSG = {
  HANDSHAKE: 0x0001,
  MOVE: 3001,
  CHAT: 2001,
  ATTACK: 1001,
  QUERY: 4001,
  STATS: 5001,
  CHAT_ACK: 7001,
  CHAT_BROADCAST: 7002,
  POSITION_UPDATE: 8001,
  PLAYER_ENTER: 8002,
  PLAYER_LEAVE: 8003,
  PLAYER_LIST: 9001,
  BATTLE: 6001,
};

// ── CRC32 ──────────────────────────────────────────────────────────
const crc32Table = (() => {
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let j = 0; j < 8; j++) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[i] = c >>> 0;
  }
  return table;
})();

function crc32(buf) {
  let crc = 0xffffffff;
  for (let i = 0; i < buf.length; i++) {
    crc = crc32Table[(crc ^ buf[i]) & 0xff] ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

// ── AES-256-GCM ────────────────────────────────────────────────────
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
  const bodyLen = encrypted.length;
  const header = Buffer.alloc(HEADER_SIZE);
  header[0] = MAGIC[0];
  header[1] = MAGIC[1];
  header[2] = PROTOCOL_VERSION;
  header[3] = 0;
  header.writeUInt16BE(msgId, 4);
  header.writeUInt16BE(bodyLen, 6);
  header.writeUInt32BE(crc32(encrypted), 8);
  header.writeUInt32BE(0, 12);
  return Buffer.concat([header, encrypted]);
}

function encodeHandshake(uid, token) {
  const payload = JSON.stringify({
    uid: uid,
    token: token,
    version: PROTOCOL_VERSION,
    timestamp: Math.floor(Date.now() / 1000),
  });
  return encodePacket(0x0001, Buffer.from(payload, "utf8"));
}

class PacketDecoder {
  constructor() {
    this.buffer = Buffer.alloc(0);
  }
  feed(data) {
    this.buffer = Buffer.concat([this.buffer, data]);
  }
  decodeAll() {
    const packets = [];
    while (this.buffer.length >= HEADER_SIZE) {
      if (this.buffer[0] !== MAGIC[0] || this.buffer[1] !== MAGIC[1]) {
        throw new Error("magic mismatch");
      }
      const msgId = this.buffer.readUInt16BE(4);
      const bodyLen = this.buffer.readUInt16BE(6);
      const crc = this.buffer.readUInt32BE(8);
      if (bodyLen > MAX_BODY_SIZE) throw new Error("body too large");
      const totalLen = HEADER_SIZE + bodyLen;
      if (this.buffer.length < totalLen) break;
      const body = this.buffer.subarray(HEADER_SIZE, totalLen);
      if (crc32(body) !== crc) throw new Error("CRC mismatch");
      const plaintext = decrypt(body);
      packets.push({ msgId, data: plaintext.toString("utf8") });
      this.buffer = this.buffer.subarray(totalLen);
    }
    return packets;
  }
}

// ── 全局统计 ────────────────────────────────────────────────────────
const stats = {
  startTime: 0,
  targetConnections: CONFIG.connections,
  connected: 0,
  disconnected: 0,
  reconnectAttempts: 0,
  totalSent: 0,
  totalReceived: 0,
  totalErrors: 0,
  // 消息类型统计
  sentByType: {},
  receivedByType: {},
  // 延迟追踪 (move 发送 -> position_update 接收)
  latencies: [],
  // 断连记录
  disconnectEvents: [],
  // 采样历史
  samples: [],
  // 崩溃检测
  crashDetected: false,
  crashTime: null,
  crashReason: null,
};

// ── 客户端 ──────────────────────────────────────────────────────────
class StressClient {
  constructor(uid) {
    this.uid = uid;
    this.socket = null;
    this.decoder = new PacketDecoder();
    this.connected = false;
    this.msgsSent = 0;
    this.msgsReceived = 0;
    this.lastSentTime = 0;
    this.msgTimer = null;
    this.connectTime = 0;
    this.pendingMoveTimestamps = new Map(); // track move latency
  }

  connect() {
    return new Promise((resolve, reject) => {
      this.socket = new net.Socket();
      this.socket.setNoDelay(true);
      const timeout = setTimeout(() => {
        this.socket.destroy();
        reject(new Error("connect timeout"));
      }, 10000);

      this.socket.connect(CONFIG.tcpPort, CONFIG.host, () => {
        clearTimeout(timeout);
        this.connected = true;
        this.connectTime = Date.now();
        stats.connected++;

        // 发送握手
        const handshake = encodeHandshake(this.uid, "test_token_123");
        this.socket.write(handshake);
        this.msgsSent++;
        stats.totalSent++;

        // 开始周期消息
        this.startMessaging();

        resolve();
      });

      this.socket.on("data", (data) => {
        this.decoder.feed(data);
        try {
          const packets = this.decoder.decodeAll();
          for (const pkt of packets) {
            this.msgsReceived++;
            stats.totalReceived++;
            stats.receivedByType[pkt.msgId] = (stats.receivedByType[pkt.msgId] || 0) + 1;

            // Track move latency: if we receive position_update for our uid
            if (pkt.msgId === MSG.POSITION_UPDATE) {
              try {
                const d = JSON.parse(pkt.data);
                if (d.uid === this.uid) {
                  // Find the earliest pending move timestamp
                  const ts = this.pendingMoveTimestamps.values().next().value;
                  if (ts) {
                    const latency = Date.now() - ts;
                    if (latency > 0 && latency < 10000) {
                      stats.latencies.push(latency);
                      if (stats.latencies.length > 10000) {
                        stats.latencies.shift();
                      }
                    }
                    // Clear oldest
                    const firstKey = this.pendingMoveTimestamps.keys().next().value;
                    this.pendingMoveTimestamps.delete(firstKey);
                  }
                }
              } catch (e) {}
            }
          }
        } catch (e) {
          stats.totalErrors++;
          this.disconnect("decode error: " + e.message);
        }
      });

      this.socket.on("error", (err) => {
        clearTimeout(timeout);
        if (!this.connected) {
          reject(err);
        }
      });

      this.socket.on("close", () => {
        clearTimeout(timeout);
        if (this.connected) {
          this.connected = false;
          stats.connected--;
          stats.disconnected++;
          stats.disconnectEvents.push({
            uid: this.uid,
            time: Date.now(),
            uptime: Date.now() - this.connectTime,
          });
          // Keep only last 100 events
          if (stats.disconnectEvents.length > 100) {
            stats.disconnectEvents.shift();
          }
        }
        this.stopMessaging();
      });
    });
  }

  startMessaging() {
    if (this.msgTimer) clearInterval(this.msgTimer);

    // 随机消息模式: 70% move, 15% chat, 10% query, 5% attack
    this.msgTimer = setInterval(() => {
      if (!this.connected) return;
      const rand = Math.random();
      let msgId, data;

      if (rand < 0.70) {
        // Move
        msgId = MSG.MOVE;
        const x = Math.floor(Math.random() * 1600);
        const y = Math.floor(Math.random() * 1200);
        data = JSON.stringify({ x, y, dir: Math.floor(Math.random() * 4) });
        this.pendingMoveTimestamps.set(Date.now(), Date.now());
        // Clean old pending timestamps (older than 5 seconds)
        const now = Date.now();
        for (const [k, v] of this.pendingMoveTimestamps) {
          if (now - v > 5000) this.pendingMoveTimestamps.delete(k);
        }
      } else if (rand < 0.85) {
        // Chat
        msgId = MSG.CHAT;
        data = JSON.stringify({ text: `stress-test-uid-${this.uid}-${Date.now()}` });
      } else if (rand < 0.95) {
        // Query
        msgId = MSG.QUERY;
        data = JSON.stringify({});
      } else {
        // Attack (target a random mob uid 20000-20015)
        msgId = MSG.ATTACK;
        const targetUid = 20000 + Math.floor(Math.random() * 16);
        data = JSON.stringify({ targetUid });
      }

      try {
        const pkt = encodePacket(msgId, Buffer.from(data, "utf8"));
        this.socket.write(pkt);
        this.msgsSent++;
        stats.totalSent++;
        stats.sentByType[msgId] = (stats.sentByType[msgId] || 0) + 1;
        this.lastSentTime = Date.now();
      } catch (e) {
        stats.totalErrors++;
      }
    }, CONFIG.msgIntervalMs + Math.floor(Math.random() * 200)); // Add jitter
  }

  stopMessaging() {
    if (this.msgTimer) {
      clearInterval(this.msgTimer);
      this.msgTimer = null;
    }
  }

  disconnect(reason) {
    this.stopMessaging();
    if (this.socket) {
      this.connected = false;
      this.socket.destroy();
      this.socket = null;
    }
  }
}

// ── HTTP 健康检查 ───────────────────────────────────────────────────
function httpGet(port, path) {
  return new Promise((resolve) => {
    const req = http.get(
      { hostname: CONFIG.host, port: port, path: path, timeout: 5000 },
      (res) => {
        let data = "";
        res.on("data", (chunk) => (data += chunk));
        res.on("end", () => {
          try {
            resolve({ status: res.statusCode, body: JSON.parse(data) });
          } catch (e) {
            resolve({ status: res.statusCode, body: data });
          }
        });
      }
    );
    req.on("error", (e) => resolve({ status: 0, error: e.message }));
    req.on("timeout", () => {
      req.destroy();
      resolve({ status: 0, error: "timeout" });
    });
  });
}

// ── 获取网关进程内存 (Windows PowerShell) ───────────────────────────
let gatewayPid = null;

function findGatewayPid() {
  try {
    // Use netstat to find PID listening on TCP port
    const output = execSync(
      `netstat -ano | findstr ":${CONFIG.tcpPort}.*LISTENING"`,
      { encoding: "utf8", timeout: 5000 }
    );
    const match = output.match(/(\d+)\s*$/m);
    if (match) {
      gatewayPid = parseInt(match[1]);
      return getProcessMem(gatewayPid);
    }
  } catch (e) {}
  return null;
}

function getProcessMem(pid) {
  try {
    // Use PowerShell instead of deprecated wmic
    const output = execSync(
      `powershell -NoProfile -Command "(Get-Process -Id ${pid}).WorkingSet64"`,
      { encoding: "utf8", timeout: 5000 }
    ).trim();
    const memBytes = parseInt(output);
    if (memBytes > 0) {
      return { pid, memBytes };
    }
  } catch (e) {}
  return { pid, memBytes: 0 };
}

// ── 采样 ────────────────────────────────────────────────────────────
async function sample(elapsedSec) {
  // HTTP 健康检查
  const health = await httpGet(CONFIG.httpPort, "/health");

  // 网关进程内存
  let procMem = null;
  if (gatewayPid) {
    procMem = getProcessMem(gatewayPid);
  } else {
    procMem = findGatewayPid();
  }

  // 计算延迟统计
  const latencies = stats.latencies;
  let latP50 = 0, latP99 = 0, latAvg = 0;
  if (latencies.length > 0) {
    const sorted = [...latencies].sort((a, b) => a - b);
    latP50 = sorted[Math.floor(sorted.length * 0.5)];
    latP99 = sorted[Math.floor(sorted.length * 0.99)];
    latAvg = sorted.reduce((a, b) => a + b, 0) / sorted.length;
  }

  const sample = {
    elapsedSec,
    timestamp: new Date().toISOString(),
    connected: stats.connected,
    targetConnections: CONFIG.connections,
    totalSent: stats.totalSent,
    totalReceived: stats.totalReceived,
    totalErrors: stats.totalErrors,
    disconnectCount: stats.disconnected,
    gatewayOnline: health.status === 200,
    gatewayOnlineCount: health.body?.online_count || 0,
    gatewayUptime: health.body?.uptime_secs || 0,
    processMemMB: procMem ? (procMem.memBytes / 1024 / 1024).toFixed(2) : 0,
    processPid: procMem?.pid || gatewayPid || 0,
    latP50,
    latP99,
    latAvg: latAvg.toFixed(1),
    latCount: latencies.length,
    // 计算本轮消息速率
    msgRate: 0, // 会在外面计算
  };

  stats.samples.push(sample);
  return sample;
}

// ── 日志 ────────────────────────────────────────────────────────────
function log(msg) {
  const ts = new Date().toISOString();
  const line = `[${ts}] ${msg}`;
  console.log(line);
  if (logStream) logStream.write(line + "\n");
}

let logStream = null;

// ── 生成 HTML 报告 ──────────────────────────────────────────────────
function generateHTMLReport() {
  const endTime = Date.now();
  const totalDurationSec = Math.floor((endTime - stats.startTime) / 1000);

  // 内存趋势数据
  const memData = stats.samples
    .filter((s) => s.processMemMB > 0)
    .map((s) => ({ x: s.elapsedSec, y: parseFloat(s.processMemMB) }));

  // 连接数趋势
  const connData = stats.samples.map((s) => ({ x: s.elapsedSec, y: s.connected }));

  // 延迟趋势
  const latData = stats.samples
    .filter((s) => s.latCount > 0)
    .map((s) => ({ x: s.elapsedSec, p50: s.latP50, p99: s.latP99 }));

  // 判断是否通过
  const lastMem = memData.length > 0 ? memData[memData.length - 1].y : 0;
  const firstMem = memData.length > 0 ? memData[0].y : 0;
  const memGrowth = lastMem - firstMem;
  const memGrowthPct = firstMem > 0 ? ((memGrowth / firstMem) * 100).toFixed(2) : 0;
  const noCrash = !stats.crashDetected;
  const noMemoryLeak = memGrowthPct < 20; // 内存增长不超过 20%
  const msgLossRate = stats.totalSent > 0
    ? ((1 - stats.totalReceived / Math.max(stats.totalSent, 1)) * 100).toFixed(2)
    : 0;

  const passed = noCrash && noMemoryLeak;
  const html = `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Rust MMO Gateway - Stability Test Report</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #f5f5f5; color: #333; }
  .container { max-width: 1200px; margin: 0 auto; padding: 20px; }
  .header { background: linear-gradient(135deg, #1a1a2e, #16213e); color: white; padding: 30px; border-radius: 12px; margin-bottom: 24px; }
  .header h1 { font-size: 28px; margin-bottom: 8px; }
  .header .subtitle { font-size: 14px; opacity: 0.8; }
  .status-badge { display: inline-block; padding: 6px 16px; border-radius: 20px; font-weight: bold; font-size: 14px; margin-top: 12px; }
  .status-pass { background: #4caf50; color: white; }
  .status-fail { background: #f44336; color: white; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; margin-bottom: 24px; }
  .card { background: white; padding: 20px; border-radius: 10px; box-shadow: 0 2px 8px rgba(0,0,0,0.08); }
  .card .label { font-size: 12px; color: #888; text-transform: uppercase; margin-bottom: 8px; }
  .card .value { font-size: 28px; font-weight: 700; color: #1a1a2e; }
  .card .unit { font-size: 14px; font-weight: 400; color: #888; }
  .card .detail { font-size: 12px; color: #666; margin-top: 4px; }
  .section { background: white; padding: 24px; border-radius: 10px; box-shadow: 0 2px 8px rgba(0,0,0,0.08); margin-bottom: 24px; }
  .section h2 { font-size: 18px; margin-bottom: 16px; color: #1a1a2e; border-bottom: 2px solid #e0e0e0; padding-bottom: 8px; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th, td { padding: 8px 12px; text-align: left; border-bottom: 1px solid #eee; }
  th { background: #f8f8f8; font-weight: 600; color: #555; }
  tr:hover { background: #f9f9f9; }
  .chart-container { position: relative; height: 300px; margin: 16px 0; }
  .bar-chart { display: flex; align-items: flex-end; height: 200px; gap: 2px; padding: 10px 0; border-bottom: 2px solid #ddd; border-left: 2px solid #ddd; }
  .bar { flex: 1; min-height: 2px; border-radius: 2px 2px 0 0; transition: height 0.3s; position: relative; }
  .bar:hover { opacity: 0.8; }
  .bar:hover::after { content: attr(data-value); position: absolute; bottom: 100%; left: 50%; transform: translateX(-50%); background: #333; color: white; padding: 2px 6px; border-radius: 4px; font-size: 10px; white-space: nowrap; }
  .chart-labels { display: flex; gap: 2px; font-size: 10px; color: #888; margin-top: 4px; }
  .chart-labels span { flex: 1; text-align: center; }
  .check-item { display: flex; align-items: center; padding: 12px 0; border-bottom: 1px solid #eee; }
  .check-icon { width: 24px; height: 24px; border-radius: 50%; display: flex; align-items: center; justify-content: center; margin-right: 12px; font-weight: bold; color: white; }
  .check-pass { background: #4caf50; }
  .check-fail { background: #f44336; }
  .check-text { flex: 1; }
  .check-text .title { font-weight: 600; }
  .check-text .desc { font-size: 12px; color: #888; }
  .summary { background: linear-gradient(135deg, #1a1a2e, #16213e); color: white; padding: 24px; border-radius: 10px; margin-bottom: 24px; }
  .summary h2 { color: white; border-color: rgba(255,255,255,0.2); }
  .footer { text-align: center; padding: 20px; color: #888; font-size: 12px; }
</style>
</head>
<body>
<div class="container">
  <div class="header">
    <h1>Rust MMO Gateway - Stability Test Report</h1>
    <div class="subtitle">Duration: ${totalDurationSec}s (${Math.floor(totalDurationSec/3600)}h ${Math.floor((totalDurationSec%3600)/60)}m ${totalDurationSec%60}s) | Target: ${CONFIG.connections} connections | Start: ${new Date(stats.startTime).toISOString()}</div>
    <div class="status-badge ${passed ? "status-pass" : "status-fail"}">${passed ? "PASS" : "FAIL"}</div>
  </div>

  <div class="grid">
    <div class="card">
      <div class="label">Final Connections</div>
      <div class="value">${stats.connected}<span class="unit"> / ${CONFIG.connections}</span></div>
      <div class="detail">Disconnected: ${stats.disconnected}</div>
    </div>
    <div class="card">
      <div class="label">Total Messages Sent</div>
      <div class="value">${stats.totalSent.toLocaleString()}</div>
      <div class="detail">Errors: ${stats.totalErrors}</div>
    </div>
    <div class="card">
      <div class="label">Total Messages Received</div>
      <div class="value">${stats.totalReceived.toLocaleString()}</div>
      <div class="detail">Loss rate: ${msgLossRate}%</div>
    </div>
    <div class="card">
      <div class="label">Avg Message Rate</div>
      <div class="value">${stats.samples.length > 0 ? Math.round(stats.samples.reduce((a,s) => a + s.msgRate, 0) / stats.samples.length) : 0}<span class="unit"> msg/s</span></div>
      <div class="detail">Peak: ${stats.samples.length > 0 ? Math.max(...stats.samples.map(s => s.msgRate)) : 0} msg/s</div>
    </div>
    <div class="card">
      <div class="label">Latency P99</div>
      <div class="value">${stats.samples.filter(s => s.latCount > 0).length > 0 ? Math.max(...stats.samples.filter(s => s.latCount > 0).map(s => s.latP99)) : 0}<span class="unit"> ms</span></div>
      <div class="detail">P50 avg: ${stats.samples.filter(s => s.latCount > 0).length > 0 ? Math.round(stats.samples.filter(s => s.latCount > 0).reduce((a,s) => a + s.latP50, 0) / stats.samples.filter(s => s.latCount > 0).length) : 0} ms</div>
    </div>
    <div class="card">
      <div class="label">Memory (Final)</div>
      <div class="value">${memData.length > 0 ? memData[memData.length-1].y : 0}<span class="unit"> MB</span></div>
      <div class="detail">Growth: ${memGrowth >= 0 ? "+" : ""}${memGrowth.toFixed(2)} MB (${memGrowthPct}%)</div>
    </div>
  </div>

  <div class="section">
    <h2>Verification Checklist</h2>
    <div class="check-item">
      <div class="check-icon ${noCrash ? "check-pass" : "check-fail"}">${noCrash ? "&#10003;" : "&#10007;"}</div>
      <div class="check-text">
        <div class="title">No Crash / Panic</div>
        <div class="desc">${noCrash ? "Gateway process stable throughout entire test duration" : `Crash detected at ${stats.crashTime || "unknown"}: ${stats.crashReason || "unknown"}`}</div>
      </div>
    </div>
    <div class="check-item">
      <div class="check-icon ${noMemoryLeak ? "check-pass" : "check-fail"}">${noMemoryLeak ? "&#10003;" : "&#10007;"}</div>
      <div class="check-text">
        <div class="title">No Memory Leak</div>
        <div class="desc">Memory growth: ${memGrowthPct}% (${memGrowth >= 0 ? "+" : ""}${memGrowth.toFixed(2)} MB) ${noMemoryLeak ? "- Within acceptable range" : "- Exceeds 20% threshold"}</div>
      </div>
    </div>
    <div class="check-item">
      <div class="check-icon ${stats.disconnected === 0 ? "check-pass" : "check-fail"}">${stats.disconnected === 0 ? "&#10003;" : "&#10007;"}</div>
      <div class="check-text">
        <div class="title">No Unexpected Disconnects</div>
        <div class="desc">${stats.disconnected === 0 ? "All connections maintained throughout test" : `${stats.disconnected} disconnect events recorded`}</div>
      </div>
    </div>
    <div class="check-item">
      <div class="check-icon ${parseFloat(msgLossRate) < 5 ? "check-pass" : "check-fail"}">${parseFloat(msgLossRate) < 5 ? "&#10003;" : "&#10007;"}</div>
      <div class="check-text">
        <div class="title">Low Message Loss Rate</div>
        <div class="desc">Loss rate: ${msgLossRate}% ${parseFloat(msgLossRate) < 5 ? "- Within acceptable range (<5%)" : "- Exceeds 5% threshold"}</div>
      </div>
    </div>
  </div>

  <div class="section">
    <h2>Memory Trend (MB over time)</h2>
    <div class="chart-container">
      <div class="bar-chart">
        ${memData.map((d, i) => {
          const maxMem = Math.max(...memData.map(m => m.y), 1);
          const heightPct = (d.y / maxMem) * 100;
          const memColor = d.y > maxMem * 0.9 ? "#f44336" : d.y > maxMem * 0.7 ? "#ff9800" : "#4caf50";
          return `<div class="bar" style="height: ${heightPct}%; background: ${memColor};" data-value="${d.y}MB @ ${Math.floor(d.x/60)}m"></div>`;
        }).join("")}
      </div>
      <div class="chart-labels">
        ${memData.filter((_, i) => i % Math.max(1, Math.floor(memData.length / 10)) === 0).map(d => `<span>${Math.floor(d.x/60)}m</span>`).join("")}
      </div>
    </div>
    <table>
      <tr><th>Time</th><th>Memory (MB)</th><th>Delta (MB)</th></tr>
      ${memData.map((d, i) => {
        const delta = i > 0 ? d.y - memData[i-1].y : 0;
        return `<tr><td>${Math.floor(d.x/60)}m ${d.x%60}s</td><td>${d.y}</td><td style="color: ${delta > 0 ? "#f44336" : delta < 0 ? "#4caf50" : "#888"}">${delta > 0 ? "+" : ""}${delta.toFixed(2)}</td></tr>`;
      }).join("")}
    </table>
  </div>

  <div class="section">
    <h2>Connection & Message Rate Trend</h2>
    <table>
      <tr><th>Time</th><th>Connected</th><th>Gateway Online</th><th>Msg Rate</th><th>Sent</th><th>Received</th><th>Errors</th><th>Lat P50</th><th>Lat P99</th><th>Mem (MB)</th></tr>
      ${stats.samples.map(s => `
        <tr>
          <td>${Math.floor(s.elapsedSec/60)}m ${s.elapsedSec%60}s</td>
          <td>${s.connected}/${s.targetConnections}</td>
          <td style="color: ${s.gatewayOnline ? "#4caf50" : "#f44336"}">${s.gatewayOnline ? "YES" : "NO"}</td>
          <td>${s.msgRate} /s</td>
          <td>${s.totalSent.toLocaleString()}</td>
          <td>${s.totalReceived.toLocaleString()}</td>
          <td style="color: ${s.totalErrors > 0 ? "#f44336" : "#4caf50"}">${s.totalErrors}</td>
          <td>${s.latP50} ms</td>
          <td>${s.latP99} ms</td>
          <td>${s.processMemMB}</td>
        </tr>
      `).join("")}
    </table>
  </div>

  ${stats.disconnectEvents.length > 0 ? `
  <div class="section">
    <h2>Disconnect Events (Last ${Math.min(stats.disconnectEvents.length, 50)})</h2>
    <table>
      <tr><th>UID</th><th>Time</th><th>Uptime (s)</th></tr>
      ${stats.disconnectEvents.slice(-50).map(e => `<tr><td>${e.uid}</td><td>${new Date(e.time).toISOString()}</td><td>${Math.floor(e.uptime/1000)}s</td></tr>`).join("")}
    </table>
  </div>
  ` : ""}

  <div class="section">
    <h2>Message Type Statistics</h2>
    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 24px;">
      <div>
        <h3 style="font-size: 14px; margin-bottom: 8px;">Sent</h3>
        <table>
          <tr><th>Msg ID</th><th>Name</th><th>Count</th></tr>
          ${Object.entries(stats.sentByType).sort((a,b) => b[1]-a[1]).map(([id, count]) => {
            const name = Object.entries(MSG).find(([_, v]) => v === parseInt(id))?.[0] || "Unknown";
            return `<tr><td>${id}</td><td>${name}</td><td>${count.toLocaleString()}</td></tr>`;
          }).join("")}
        </table>
      </div>
      <div>
        <h3 style="font-size: 14px; margin-bottom: 8px;">Received</h3>
        <table>
          <tr><th>Msg ID</th><th>Name</th><th>Count</th></tr>
          ${Object.entries(stats.receivedByType).sort((a,b) => b[1]-a[1]).map(([id, count]) => {
            const name = Object.entries(MSG).find(([_, v]) => v === parseInt(id))?.[0] || "Unknown";
            return `<tr><td>${id}</td><td>${name}</td><td>${count.toLocaleString()}</td></tr>`;
          }).join("")}
        </table>
      </div>
    </div>
  </div>

  <div class="summary">
    <h2>Conclusion</h2>
    <p style="margin-top: 12px; line-height: 1.8;">
      Test Duration: <strong>${Math.floor(totalDurationSec/3600)}h ${Math.floor((totalDurationSec%3600)/60)}m ${totalDurationSec%60}s</strong><br>
      Target Connections: <strong>${CONFIG.connections}</strong><br>
      Final Connected: <strong>${stats.connected}</strong><br>
      Total Messages: <strong>${stats.totalSent.toLocaleString()} sent / ${stats.totalReceived.toLocaleString()} received</strong><br>
      Message Loss Rate: <strong>${msgLossRate}%</strong><br>
      Memory Growth: <strong>${memGrowthPct}% (${memGrowth >= 0 ? "+" : ""}${memGrowth.toFixed(2)} MB)</strong><br>
      Crash Detected: <strong>${stats.crashDetected ? "YES" : "NO"}</strong><br>
      <br>
      Result: <strong style="font-size: 18px;">${passed ? "PASS - Stability test passed all checks" : "FAIL - See issues above"}</strong>
    </p>
  </div>

  <div class="footer">
    Generated at ${new Date().toISOString()} | Rust MMO Gateway Stability Test
  </div>
</div>
</body>
</html>`;

  return html;
}

// ── 主流程 ──────────────────────────────────────────────────────────
async function main() {
  console.log("");
  console.log("======================================================");
  console.log("  Rust MMO Gateway - Stability Stress Test");
  console.log("======================================================");
  console.log(`  Target:    ${CONFIG.connections} connections`);
  console.log(`  Duration:  ${CONFIG.durationMin} minutes (${DURATION_SEC}s)`);
  console.log(`  Gateway:   ${CONFIG.host}:${CONFIG.tcpPort} (HTTP ${CONFIG.httpPort})`);
  console.log(`  Msg Rate:  ~${Math.round(CONFIG.connections * (1000 / CONFIG.msgInterval))} msg/s expected`);
  console.log("======================================================");
  console.log("");

  // 创建报告目录
  if (!fs.existsSync(CONFIG.reportDir)) {
    fs.mkdirSync(CONFIG.reportDir, { recursive: true });
  }

  // 日志文件
  const logFile = path.join(CONFIG.reportDir, `stability_${Date.now()}.log`);
  logStream = fs.createWriteStream(logFile, { flags: "w" });

  stats.startTime = Date.now();
  log(`Stability test started. Log file: ${logFile}`);

  // 先检测网关是否在线
  const healthCheck = await httpGet(CONFIG.httpPort, "/health");
  if (healthCheck.status !== 200) {
    log(`ERROR: Gateway not reachable at ${CONFIG.host}:${CONFIG.httpPort}. Status: ${healthCheck.status}`);
    log("Please start the gateway first: cd E:/ai/rust-mmo-gate && cargo run --release");
    process.exit(1);
  }
  log(`Gateway is online. Online count: ${healthCheck.body?.online_count || 0}`);

  // 查找网关进程 PID
  const procInfo = findGatewayPid();
  if (procInfo) {
    gatewayPid = procInfo.pid;
    log(`Gateway PID: ${gatewayPid}, Memory: ${(procInfo.memBytes / 1024 / 1024).toFixed(2)} MB`);
  } else {
    log("WARNING: Could not find gateway process PID. Memory tracking may be limited.");
  }

  // ── 阶段 1: 建立连接 ──
  log(`Phase 1: Establishing ${CONFIG.connections} connections...`);
  const clients = [];
  let connectSuccess = 0;
  let connectFail = 0;
  const connectStart = Date.now();

  for (let batchStart = 0; batchStart < CONFIG.connections; batchStart += CONFIG.connectBatchSize) {
    const batchEnd = Math.min(batchStart + CONFIG.connectBatchSize, CONFIG.connections);
    const batchSize = batchEnd - batchStart;
    const batchPromises = [];

    for (let i = batchStart; i < batchEnd; i++) {
      const uid = 50000 + i; // Use uid range 50000+ for stress test
      const client = new StressClient(uid);
      clients.push(client);
      batchPromises.push(
        client.connect().then(() => {
          connectSuccess++;
        }).catch((e) => {
          connectFail++;
        })
      );
    }

    await Promise.all(batchPromises);

    const progress = ((batchEnd / CONFIG.connections) * 100).toFixed(1);
    const elapsed = ((Date.now() - connectStart) / 1000).toFixed(1);
    log(`  Progress: ${batchEnd}/${CONFIG.connections} (${progress}%) - Success: ${connectSuccess}, Fail: ${connectFail} - ${elapsed}s`);

    if (batchEnd < CONFIG.connections && CONFIG.connectDelayMs > 0) {
      await new Promise((r) => setTimeout(r, CONFIG.connectDelayMs));
    }
  }

  const connectDuration = ((Date.now() - connectStart) / 1000).toFixed(1);
  log(`Phase 1 complete: ${connectSuccess} connected, ${connectFail} failed in ${connectDuration}s`);
  log(`  Connect rate: ${(connectSuccess / parseFloat(connectDuration)).toFixed(0)} conn/s`);

  // ── 阶段 2: 持续运行 + 采样 ──
  log(`Phase 2: Running stability test for ${CONFIG.durationMin} minutes...`);
  log(`  Sampling every ${SAMPLE_INTERVAL_SEC}s`);

  let lastSampleSent = stats.totalSent;
  let lastSampleReceived = stats.totalReceived;
  let lastSampleTime = Date.now();

  const sampleTimer = setInterval(async () => {
    const elapsedSec = Math.floor((Date.now() - stats.startTime) / 1000);
    const now = Date.now();
    const intervalSec = (now - lastSampleTime) / 1000;

    const s = await sample(elapsedSec);
    s.msgRate = Math.round((stats.totalSent - lastSampleSent) / intervalSec);
    s.recvRate = Math.round((stats.totalReceived - lastSampleReceived) / intervalSec);

    lastSampleSent = stats.totalSent;
    lastSampleReceived = stats.totalReceived;
    lastSampleTime = now;

    log(
      `[${Math.floor(elapsedSec/60)}m${String(elapsedSec%60).padStart(2,'0')}s] ` +
      `Conn: ${s.connected}/${s.targetConnections} | ` +
      `Gateway: ${s.gatewayOnline ? "UP" : "DOWN"} (${s.gatewayOnlineCount} online) | ` +
      `Msg: ${s.msgRate}/s sent, ${s.recvRate}/s recv | ` +
      `Total: ${s.totalSent} sent, ${s.totalReceived} recv | ` +
      `Err: ${s.totalErrors} | ` +
      `Disc: ${s.disconnectCount} | ` +
      `Lat: P50=${s.latP50}ms P99=${s.latP99}ms (${s.latCount} samples) | ` +
      `Mem: ${s.processMemMB}MB`
    );

    // 检测崩溃
    if (!s.gatewayOnline && !stats.crashDetected) {
      stats.crashDetected = true;
      stats.crashTime = new Date().toISOString();
      stats.crashReason = "Gateway health check failed";
      log(`!!! CRASH DETECTED at ${stats.crashTime} !!!`);
    }

    // 输出内存泄漏警告
    if (stats.samples.length >= 5) {
      const recent = stats.samples.slice(-5).filter((s) => s.processMemMB > 0);
      if (recent.length >= 3) {
        const memTrend = recent.map((s) => parseFloat(s.processMemMB));
        const isIncreasing = memTrend.every((m, i) => i === 0 || m >= memTrend[i - 1] - 0.5);
        if (isIncreasing) {
          const growth = memTrend[memTrend.length - 1] - memTrend[0];
          if (growth > 5) {
            log(`  WARNING: Memory continuously increasing (+${growth.toFixed(2)}MB in last ${recent.length} samples)`);
          }
        }
      }
    }
  }, SAMPLE_INTERVAL_SEC * 1000);

  // 等待测试结束
  await new Promise((r) => setTimeout(r, DURATION_SEC * 1000));
  clearInterval(sampleTimer);

  // ── 阶段 3: 最终采样 ──
  log("Phase 3: Final sampling and cleanup...");
  const finalElapsed = Math.floor((Date.now() - stats.startTime) / 1000);
  const finalSample = await sample(finalElapsed);
  finalSample.msgRate = Math.round((stats.totalSent - lastSampleSent) / ((Date.now() - lastSampleTime) / 1000));
  finalSample.recvRate = Math.round((stats.totalReceived - lastSampleReceived) / ((Date.now() - lastSampleTime) / 1000));

  // 断开所有连接
  log("Disconnecting all clients...");
  for (const client of clients) {
    client.disconnect("test complete");
  }

  // ── 生成报告 ──
  log("Generating HTML report...");
  const html = generateHTMLReport();
  const reportFile = path.join(CONFIG.reportDir, `stability_report_${Date.now()}.html`);
  fs.writeFileSync(reportFile, html);

  log("");
  log("======================================================");
  log("  Stability Test Complete");
  log("======================================================");
  log(`  Duration:        ${finalElapsed}s (${Math.floor(finalElapsed/3600)}h ${Math.floor((finalElapsed%3600)/60)}m ${finalElapsed%60}s)`);
  log(`  Connected:       ${stats.connected}/${CONFIG.connections}`);
  log(`  Total Sent:      ${stats.totalSent.toLocaleString()}`);
  log(`  Total Received:  ${stats.totalReceived.toLocaleString()}`);
  log(`  Total Errors:    ${stats.totalErrors}`);
  log(`  Disconnects:     ${stats.disconnected}`);
  log(`  Crash Detected:  ${stats.crashDetected}`);
  log(`  Memory (final):  ${finalSample.processMemMB}MB`);
  log(`  Report:          ${reportFile}`);
  log(`  Log:             ${logFile}`);
  log("======================================================");

  if (logStream) {
    logStream.end();
  }

  process.exit(0);
}

main().catch((e) => {
  console.error("Fatal error:", e);
  if (logStream) logStream.end();
  process.exit(1);
});
