/**
 * Throughput Benchmark - Rust MMO Gateway
 *
 * 逐步加大消息速率, 测量网关极限吞吐能力
 * 门禁: >=80000 pps, 丢失率=0, P99<100ms
 *
 * Usage:
 *   node test_throughput.js --connections 100 --steps 1k,10k,50k,80k,100k
 *   node test_throughput.js --connections 500 --steps 1k,10k,50k,80k,100k,120k
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
  connections: parseInt(getArg("connections", "100")),
  steps: getArg("steps", "1k,10k,50k,80k,100k").split(",").map(parseRate),
  stepDurationSec: parseInt(getArg("stepDuration", "30")),
  aesKey: getArg("aesKey", "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"),
  reportDir: path.join(__dirname, "stability_reports"),
};

function parseRate(s) {
  s = s.trim().toLowerCase();
  if (s.endsWith("k")) return parseInt(s) * 1000;
  if (s.endsWith("m")) return parseInt(s) * 1000000;
  return parseInt(s);
}

// ── CRC32 + AES (same as stability test) ───────────────────────────
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
const MSG_QUERY = 4001;

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

function encodeHandshake(uid) {
  return encodePacket(0x0001, Buffer.from(JSON.stringify({
    uid, token: "test_token_123", version: PROTOCOL_VERSION, timestamp: Math.floor(Date.now() / 1000),
  }), "utf8"));
}

// Pre-build query packet template (small packet for throughput)
const queryPayload = Buffer.from(JSON.stringify({}), "utf8");

// Pre-build a pool of encrypted packets to avoid per-message AES overhead
const PACKET_POOL_SIZE = 2000;
const packetPool = [];
for (let i = 0; i < PACKET_POOL_SIZE; i++) {
  const payload = Buffer.from(JSON.stringify({ _ts: 0, _seq: i }), "utf8");
  packetPool.push(encodePacket(MSG_QUERY, payload));
}
let poolIdx = 0;

// ── HTTP 健康检查 ───────────────────────────────────────────────────
function httpGet(urlPath) {
  return new Promise((resolve) => {
    const req = http.get({ hostname: CONFIG.host, port: CONFIG.httpPort, path: urlPath, timeout: 5000 }, (res) => {
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

async function getProcessMemMB(pid) {
  return new Promise((resolve) => {
    exec(`powershell -NoProfile -Command "(Get-Process -Id ${pid}).WorkingSet64"`, { timeout: 8000 }, (err, stdout) => {
      if (err) return resolve(0);
      const memBytes = parseInt(stdout.trim());
      resolve(memBytes > 0 ? memBytes / 1024 / 1024 : 0);
    });
  });
}

function findGatewayPid() {
  return new Promise((resolve) => {
    exec(`netstat -ano | findstr ":${CONFIG.tcpPort}.*LISTENING"`, { timeout: 5000 }, (err, stdout) => {
      if (err) return resolve(null);
      const match = stdout.match(/(\d+)\s*$/m);
      resolve(match ? parseInt(match[1]) : null);
    });
  });
}

// ── Throughput Client ──────────────────────────────────────────────
class ThroughputClient {
  constructor(uid) {
    this.uid = uid;
    this.socket = null;
    this.decoder = null;
    this.connected = false;
    this.sendSeq = 0;
    this.recvSeq = 0;
    this.latencies = [];
  }

  connect() {
    return new Promise((resolve, reject) => {
      this.socket = new net.Socket();
      this.socket.setNoDelay(true);

      const timeout = setTimeout(() => { this.socket.destroy(); reject(new Error("timeout")); }, 10000);

      this.socket.connect(CONFIG.tcpPort, CONFIG.host, () => {
        clearTimeout(timeout);
        this.connected = true;
        this.decoder = { buffer: Buffer.alloc(0) };
        // Set buffer sizes after connection is established
        try {
          this.socket.setSendBufferSize(256 * 1024);
          this.socket.setRecvBufferSize(256 * 1024);
        } catch {}
        const hs = encodeHandshake(this.uid);
        this.socket.write(hs);
        resolve();
      });

      this.socket.on("data", (data) => {
        if (!this.decoder) return;
        this.decoder.buffer = Buffer.concat([this.decoder.buffer, data]);
        while (this.decoder.buffer.length >= HEADER_SIZE) {
          try {
            if (this.decoder.buffer[0] !== MAGIC[0] || this.decoder.buffer[1] !== MAGIC[1]) throw new Error("magic");
            const bodyLen = this.decoder.buffer.readUInt16BE(6);
            if (bodyLen > MAX_BODY_SIZE) throw new Error("too large");
            const totalLen = HEADER_SIZE + bodyLen;
            if (this.decoder.buffer.length < totalLen) break;
            // Skip CRC check and decryption for throughput — just count packets
            // (gateway already verified CRC; echo mode returns same packet)
            this.recvSeq++;
            // Only decrypt every 100th packet for latency (timestamped ones)
            // Check if this might be a timestamped packet (body size > 20 = has _ts field)
            if (bodyLen > 20 && this.recvSeq % 100 === 0) {
              try {
                const body = this.decoder.buffer.subarray(HEADER_SIZE, totalLen);
                const plaintext = decrypt(body);
                const d = JSON.parse(plaintext.toString("utf8"));
                if (d._ts) {
                  const lat = Date.now() - d._ts;
                  if (lat > 0 && lat < 10000) this.latencies.push(lat);
                }
              } catch {}
            }
            this.decoder.buffer = this.decoder.buffer.subarray(totalLen);
          } catch {
            this.decoder.buffer = Buffer.alloc(0);
            break;
          }
        }
      });

      this.socket.on("error", () => { this.connected = false; });
      this.socket.on("close", () => { this.connected = false; });
    });
  }

  send() {
    if (!this.connected) return false;
    this.sendSeq++;
    // Every 100th message, send a timestamped packet for latency measurement
    if (this.sendSeq % 100 === 0) {
      const payload = Buffer.from(JSON.stringify({ _ts: Date.now(), _seq: this.sendSeq }), "utf8");
      const pkt = encodePacket(MSG_QUERY, payload);
      return this.socket.write(pkt);
    }
    // Use pre-built packet from pool (avoids per-message AES encryption overhead)
    const pkt = packetPool[poolIdx];
    poolIdx = (poolIdx + 1) % PACKET_POOL_SIZE;
    return this.socket.write(pkt);
  }

  disconnect() {
    if (this.socket) { this.socket.destroy(); this.socket = null; }
    this.connected = false;
  }
}

// ── Main ───────────────────────────────────────────────────────────
async function main() {
  console.log("============================================================");
  console.log("  Rust MMO Gateway - Throughput Benchmark");
  console.log("============================================================");
  console.log(`  Connections: ${CONFIG.connections}`);
  console.log(`  Steps: ${CONFIG.steps.map(s => (s >= 1000 ? (s / 1000) + "K" : s) + " msg/s").join(" -> ")}`);
  console.log(`  Step duration: ${CONFIG.stepDurationSec}s`);
  console.log(`  Gate: ${CONFIG.host}:${CONFIG.tcpPort} (HTTP :${CONFIG.httpPort})`);
  console.log("============================================================\n");

  // Phase 1: Connect all clients
  console.log(`[Phase 1] Connecting ${CONFIG.connections} clients...`);
  const clients = [];
  const connectBatch = 50;
  let connected = 0;
  const connectStart = Date.now();

  for (let i = 0; i < CONFIG.connections; i++) {
    const c = new ThroughputClient(100000 + i);
    clients.push(c);
  }

  for (let i = 0; i < clients.length; i += connectBatch) {
    const batch = clients.slice(i, i + connectBatch);
    await Promise.all(batch.map(async (c) => {
      try { await c.connect(); connected++; } catch {}
    }));
    if ((i + connectBatch) % 100 === 0 || i + connectBatch >= clients.length) {
      console.log(`  Connected: ${connected}/${CONFIG.connections}`);
    }
  }

  const connectSec = ((Date.now() - connectStart) / 1000).toFixed(1);
  console.log(`[Phase 1] Done: ${connected}/${CONFIG.connections} connected in ${connectSec}s\n`);

  if (connected < CONFIG.connections * 0.95) {
    console.log("!!! Too many connection failures, aborting.");
    process.exit(1);
  }

  // Find gateway PID for memory tracking
  const gwPid = await findGatewayPid();
  console.log(`[Gateway PID: ${gwPid || "unknown"}]\n`);

  // Phase 2: Step test
  const results = [];

  for (let stepIdx = 0; stepIdx < CONFIG.steps.length; stepIdx++) {
    const targetRate = CONFIG.steps[stepIdx];
    const rateLabel = targetRate >= 1000 ? (targetRate / 1000) + "K" : targetRate;
    console.log(`\n[Step ${stepIdx + 1}/${CONFIG.steps.length}] Target: ${rateLabel} msg/s`);

    // Calculate per-client send rate
    const activeClients = clients.filter(c => c.connected);
    if (activeClients.length === 0) {
      console.log("  No active clients, skipping step");
      continue;
    }

    const perClientRate = targetRate / activeClients.length;
    const intervalMs = Math.max(1, 1000 / perClientRate);

    // Reset counters
    let stepSent = 0;
    let stepReceived = 0;
    let stepErrors = 0;
    const stepLatencies = [];
    const stepStart = Date.now();

    // Capture pre-step recv counts
    const preRecv = activeClients.map(c => c.recvSeq);
    const preSend = activeClients.map(c => c.sendSeq);

    // Start sending
    const sender = setInterval(() => {
      for (const c of activeClients) {
        if (c.connected) {
          const ok = c.send();
          if (ok) stepSent++;
          else stepErrors++;
        }
      }
    }, intervalMs);

    // Sample every 5 seconds
    const sampler = setInterval(() => {
      const elapsed = (Date.now() - stepStart) / 1000;
      const curRecv = activeClients.reduce((s, c) => s + c.recvSeq, 0);
      const curSent = activeClients.reduce((s, c) => s + c.sendSeq, 0);
      const recvDelta = curRecv - preRecv.reduce((a, b) => a + b, 0);
      const sentDelta = curSent - preSend.reduce((a, b) => a + b, 0);
      const sendRate = (sentDelta / elapsed).toFixed(0);
      const recvRate = (recvDelta / elapsed).toFixed(0);
      const lossRate = sentDelta > 0 ? ((1 - recvDelta / sentDelta) * 100).toFixed(2) : "0";
      console.log(`  [${elapsed.toFixed(0)}s] sent: ${sendRate}/s | recv: ${recvRate}/s | loss: ${lossRate}% | active: ${activeClients.filter(c => c.connected).length}`);
    }, 5000);

    // Wait for step duration
    await new Promise(r => setTimeout(r, CONFIG.stepDurationSec * 1000));

    clearInterval(sender);
    clearInterval(sampler);

    // Wait 2s for in-flight messages
    await new Promise(r => setTimeout(r, 2000));

    // Calculate final stats
    const elapsed = (Date.now() - stepStart) / 1000;
    const totalRecv = activeClients.reduce((s, c) => s + c.recvSeq, 0);
    const totalSent = activeClients.reduce((s, c) => s + c.sendSeq, 0);
    const recvDelta = totalRecv - preRecv.reduce((a, b) => a + b, 0);
    const sentDelta = totalSent - preSend.reduce((a, b) => a + b, 0);
    const actualSendRate = sentDelta / elapsed;
    const actualRecvRate = recvDelta / elapsed;
    const lossRate = sentDelta > 0 ? (1 - recvDelta / sentDelta) : 0;

    // Collect latencies
    const allLat = activeClients.flatMap(c => c.latencies.splice(0));
    allLat.sort((a, b) => a - b);
    const p50 = allLat.length > 0 ? allLat[Math.floor(allLat.length * 0.5)] : 0;
    const p99 = allLat.length > 0 ? allLat[Math.floor(allLat.length * 0.99)] : 0;
    const p999 = allLat.length > 0 ? allLat[Math.floor(allLat.length * 0.999)] : 0;

    const memMB = gwPid ? await getProcessMemMB(gwPid) : 0;
    const activeCount = activeClients.filter(c => c.connected).length;

    const result = {
      step: stepIdx + 1,
      targetRate,
      actualSendRate: Math.round(actualSendRate),
      actualRecvRate: Math.round(actualRecvRate),
      lossRate: (lossRate * 100).toFixed(2) + "%",
      sentDelta,
      recvDelta,
      p50,
      p99,
      p999,
      memMB: memMB.toFixed(2),
      activeClients: activeCount,
      elapsed: elapsed.toFixed(1),
    };
    results.push(result);

    console.log(`\n  ── Result ──`);
    console.log(`  Target:      ${rateLabel} msg/s`);
    console.log(`  Actual send: ${result.actualSendRate.toLocaleString()} msg/s`);
    console.log(`  Actual recv: ${result.actualRecvRate.toLocaleString()} msg/s`);
    console.log(`  Loss rate:   ${result.lossRate}`);
    console.log(`  Latency:     P50=${p50}ms  P99=${p99}ms  P99.9=${p999}ms`);
    console.log(`  Memory:      ${result.memMB}MB`);
    console.log(`  Active:      ${activeCount}/${CONFIG.connections}`);

    // Check if we should stop
    if (lossRate > 0.05 || activeCount < CONFIG.connections * 0.9) {
      console.log(`\n  !!! Degradation detected, stopping benchmark`);
      break;
    }

    // Cool down 5s between steps
    if (stepIdx < CONFIG.steps.length - 1) {
      console.log(`  Cooling down 5s...`);
      await new Promise(r => setTimeout(r, 5000));
    }
  }

  // Phase 3: Summary
  console.log("\n\n============================================================");
  console.log("  THROUGHPUT BENCHMARK SUMMARY");
  console.log("============================================================");
  console.log("  Step  | Target    | Send/s    | Recv/s    | Loss   | P50  | P99  | Mem(MB)");
  console.log("  ------|-----------|-----------|-----------|--------|------|------|-------");
  for (const r of results) {
    const targetLbl = r.targetRate >= 1000 ? (r.targetRate / 1000) + "K" : r.targetRate;
    console.log(`  ${r.step}     | ${targetLbl.padEnd(9)} | ${String(r.actualSendRate).padEnd(9)} | ${String(r.actualRecvRate).padEnd(9)} | ${r.lossRate.padEnd(6)} | ${String(r.p50).padEnd(4)} | ${String(r.p99).padEnd(4)} | ${r.memMB}`);
  }
  console.log("============================================================\n");

  // Gate check
  const gate80K = results.find(r => r.targetRate === 80000);
  if (gate80K) {
    const passed = gate80K.actualRecvRate >= 80000 && parseFloat(gate80K.lossRate) === 0 && gate80K.p99 < 100;
    console.log(`  Gate check (80K pps): ${passed ? "PASS ✅" : "FAIL ❌"}`);
    console.log(`    Recv rate: ${gate80K.actualRecvRate.toLocaleString()} (need >= 80,000)`);
    console.log(`    Loss rate: ${gate80K.lossRate} (need 0%)`);
    console.log(`    P99 lat:   ${gate80K.p99}ms (need < 100ms)`);
  } else {
    console.log("  Gate check (80K pps): NOT TESTED");
  }

  // Generate HTML report
  generateReport(results);

  // Cleanup
  for (const c of clients) c.disconnect();
  console.log("\nDone. All clients disconnected.");
  process.exit(0);
}

function generateReport(results) {
  if (!fs.existsSync(CONFIG.reportDir)) fs.mkdirSync(CONFIG.reportDir, { recursive: true });
  const ts = Date.now();
  const filePath = path.join(CONFIG.reportDir, `throughput_report_${ts}.html`);

  const rows = results.map(r => {
    const targetLbl = r.targetRate >= 1000 ? (r.targetRate / 1000) + "K" : r.targetRate;
    const recvPass = r.actualRecvRate >= 80000;
    const lossPass = parseFloat(r.lossRate) === 0;
    const latPass = r.p99 < 100;
    return `<tr>
      <td>${r.step}</td>
      <td>${targetLbl}</td>
      <td>${r.actualSendRate.toLocaleString()}</td>
      <td class="${recvPass ? "pass" : "fail"}">${r.actualRecvRate.toLocaleString()}</td>
      <td class="${lossPass ? "pass" : "fail"}">${r.lossRate}</td>
      <td>${r.p50}</td>
      <td class="${latPass ? "pass" : "fail"}">${r.p99}</td>
      <td>${r.p999}</td>
      <td>${r.memMB}</td>
      <td>${r.activeClients}/${CONFIG.connections}</td>
    </tr>`;
  }).join("\n");

  const gate80K = results.find(r => r.targetRate === 80000);
  const gateStatus = gate80K
    ? (gate80K.actualRecvRate >= 80000 && parseFloat(gate80K.lossRate) === 0 && gate80K.p99 < 100 ? "PASS" : "FAIL")
    : "NOT_TESTED";

  const html = `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Throughput Benchmark Report</title>
<style>
  body { font-family: -apple-system, "Segoe UI", sans-serif; margin: 40px; background: #f8f9fa; color: #333; }
  h1 { font-size: 24px; border-bottom: 3px solid #378ADD; padding-bottom: 10px; }
  h2 { font-size: 18px; margin-top: 30px; }
  .gate { display: inline-block; padding: 8px 24px; border-radius: 8px; font-size: 20px; font-weight: 600; }
  .gate.pass { background: #d4edda; color: #155724; border: 1px solid #c3e6cb; }
  .gate.fail { background: #f8d7da; color: #721c24; border: 1px solid #f5c6cb; }
  table { border-collapse: collapse; width: 100%; margin-top: 15px; font-size: 13px; }
  th, td { border: 1px solid #dee2e6; padding: 8px 12px; text-align: center; }
  th { background: #378ADD; color: white; font-weight: 500; }
  tr:nth-child(even) { background: #f8f9fa; }
  .pass { color: #155724; font-weight: 600; }
  .fail { color: #721c24; font-weight: 600; }
  .info { background: #e8f4fd; padding: 15px; border-radius: 8px; margin: 15px 0; }
  .info div { margin: 4px 0; }
</style>
</head>
<body>
<h1>Throughput Benchmark Report</h1>
<div class="info">
  <div><b>Date:</b> ${new Date(ts).toLocaleString("zh-CN")}</div>
  <div><b>Connections:</b> ${CONFIG.connections}</div>
  <div><b>Step duration:</b> ${CONFIG.stepDurationSec}s</div>
  <div><b>Gateway:</b> ${CONFIG.host}:${CONFIG.tcpPort}</div>
</div>

<h2>Gate Check: >=80,000 pps</h2>
<div class="gate ${gateStatus === "PASS" ? "pass" : "fail"}">${gateStatus}</div>
${gate80K ? `<div style="margin-top:10px;">
  <div>Recv rate: <b>${gate80K.actualRecvRate.toLocaleString()}</b> msg/s (need >= 80,000)</div>
  <div>Loss rate: <b>${gate80K.lossRate}</b> (need 0%)</div>
  <div>P99 latency: <b>${gate80K.p99}ms</b> (need < 100ms)</div>
</div>` : ""}

<h2>Step Results</h2>
<table>
<thead><tr>
  <th>Step</th><th>Target</th><th>Send/s</th><th>Recv/s</th><th>Loss</th>
  <th>P50(ms)</th><th>P99(ms)</th><th>P99.9(ms)</th><th>Mem(MB)</th><th>Active</th>
</tr></thead>
<tbody>
${rows}
</tbody>
</table>
</body>
</html>`;

  fs.writeFileSync(filePath, html);
  console.log(`\nReport saved: ${filePath}`);
}

main().catch(e => { console.error("Fatal:", e); process.exit(1); });
