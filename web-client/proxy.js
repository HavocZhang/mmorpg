/**
 * WebSocket-to-TCP Proxy for Rust MMO Gateway
 *
 * Bridges browser WebSocket connections to the gateway's TCP protocol.
 * Implements: 16-byte header, CRC32, AES-256-GCM, handshake.
 *
 * Each WebSocket can manage multiple TCP connections to the gateway.
 *
 * Protocol (JSON over WebSocket):
 *   Client -> Proxy:
 *     {action:"connect", id, uid, token, host, port}
 *     {action:"connectBulk", count, uidStart, token, host, port, delayMs}
 *     {action:"send", id, msgId, data}
 *     {action:"broadcast", msgId, data}
 *     {action:"autoSend", intervalMs, msgId, data}
 *     {action:"stopAutoSend"}
 *     {action:"disconnect", id}
 *     {action:"disconnectAll"}
 *     {action:"getStats"}
 *   Proxy -> Client:
 *     {event:"connected", id, uid}
 *     {event:"connectFailed", id, reason}
 *     {event:"message", id, msgId, data}
 *     {event:"disconnected", id, reason}
 *     {event:"stats", ...}
 *     {event:"error", id, message}
 */

const http = require("http");
const net = require("net");
const crypto = require("crypto");
const fs = require("fs");
const path = require("path");
const { WebSocketServer } = require("ws");

// ── Protocol Constants ──────────────────────────────────────────────
const HEADER_SIZE = 16;
const MAGIC = [0x4d, 0x4d];
const PROTOCOL_VERSION = 1;
const MAX_BODY_SIZE = 8192;

// Default AES key (must match gateway .env.dev)
const DEFAULT_AES_KEY = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

// ── CRC32 (standard ISO 3309, same as crc32fast) ────────────────────
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

// ── AES-256-GCM ─────────────────────────────────────────────────────
function createCipher(keyHex) {
  const key = Buffer.from(keyHex, "hex");
  if (key.length !== 32) {
    throw new Error(`AES-256 key must be 32 bytes, got ${key.length}`);
  }
  return {
    encrypt(plaintext) {
      const nonce = crypto.randomBytes(12);
      const cipher = crypto.createCipheriv("aes-256-gcm", key, nonce);
      const encrypted = Buffer.concat([cipher.update(plaintext), cipher.final()]);
      const tag = cipher.getAuthTag();
      // Format: nonce(12) + ciphertext + tag(16) — same as Rust aes-gcm crate
      return Buffer.concat([nonce, encrypted, tag]);
    },
    decrypt(data) {
      if (data.length < 28) throw new Error("encrypted data too short");
      const nonce = data.subarray(0, 12);
      const tag = data.subarray(data.length - 16);
      const ciphertext = data.subarray(12, data.length - 16);
      const decipher = crypto.createDecipheriv("aes-256-gcm", key, nonce);
      decipher.setAuthTag(tag);
      const plaintext = Buffer.concat([decipher.update(ciphertext), decipher.final()]);
      return plaintext;
    },
  };
}

// ── Protocol Encoding ───────────────────────────────────────────────
function encodePacket(msgId, plaintext, cipher) {
  const encrypted = cipher.encrypt(plaintext);
  const bodyLen = encrypted.length;
  if (bodyLen > MAX_BODY_SIZE) {
    throw new Error(`body too large: ${bodyLen}`);
  }
  const header = Buffer.alloc(HEADER_SIZE);
  header[0] = MAGIC[0];
  header[1] = MAGIC[1];
  header[2] = PROTOCOL_VERSION;
  header[3] = 0; // reserved
  header.writeUInt16BE(msgId, 4);
  header.writeUInt16BE(bodyLen, 6);
  header.writeUInt32BE(crc32(encrypted), 8);
  header.writeUInt32BE(0, 12); // flags
  return Buffer.concat([header, encrypted]);
}

function encodeHandshake(uid, token, cipher) {
  const ts = Math.floor(Date.now() / 1000);
  const payload = JSON.stringify({
    uid: uid,
    token: token,
    version: PROTOCOL_VERSION,
    timestamp: ts,
  });
  return encodePacket(0x0001, Buffer.from(payload, "utf8"), cipher);
}

// ── Protocol Decoding ───────────────────────────────────────────────
class PacketDecoder {
  constructor(cipher) {
    this.cipher = cipher;
    this.buffer = Buffer.alloc(0);
  }

  feed(data) {
    this.buffer = Buffer.concat([this.buffer, data]);
  }

  decodeAll() {
    const packets = [];
    while (this.buffer.length >= HEADER_SIZE) {
      const header = this.buffer.subarray(0, HEADER_SIZE);
      // Check magic
      if (header[0] !== MAGIC[0] || header[1] !== MAGIC[1]) {
        throw new Error("magic mismatch");
      }
      const version = header[2];
      if (version !== PROTOCOL_VERSION) {
        throw new Error(`version mismatch: ${version}`);
      }
      const msgId = header.readUInt16BE(4);
      const bodyLen = header.readUInt16BE(6);
      const crc = header.readUInt32BE(8);

      if (bodyLen > MAX_BODY_SIZE) {
        throw new Error(`body too large: ${bodyLen}`);
      }
      const totalLen = HEADER_SIZE + bodyLen;
      if (this.buffer.length < totalLen) break; // need more data

      const body = this.buffer.subarray(HEADER_SIZE, totalLen);
      // Verify CRC
      if (crc32(body) !== crc) {
        throw new Error("CRC mismatch");
      }
      // Decrypt
      let plaintext;
      try {
        plaintext = this.cipher.decrypt(body);
      } catch (e) {
        throw new Error("decrypt failed: " + e.message);
      }

      packets.push({ msgId, data: plaintext });
      this.buffer = this.buffer.subarray(totalLen);
    }
    return packets;
  }
}

// ── Gateway Connection ─────────────────────────────────────────────
class GatewayConnection {
  constructor(id, uid, token, host, port, cipher, eventCallback) {
    this.id = id;
    this.uid = uid;
    this.token = token;
    this.host = host;
    this.port = port;
    this.cipher = cipher;
    this.eventCallback = eventCallback;
    this.socket = null;
    this.decoder = new PacketDecoder(cipher);
    this.connected = false;
    this.handshakeDone = false;
    this.msgsSent = 0;
    this.msgsReceived = 0;
    this.connectTime = 0;
  }

  connect() {
    return new Promise((resolve, reject) => {
      this.socket = new net.Socket();
      this.socket.setNoDelay(true);
      const timeout = setTimeout(() => {
        this.socket.destroy();
        reject(new Error("connect timeout"));
      }, 5000);

      this.socket.connect(this.port, this.host, () => {
        clearTimeout(timeout);
        this.connected = true;
        this.connectTime = Date.now();
        // Send handshake
        const handshakePkt = encodeHandshake(this.uid, this.token, this.cipher);
        this.socket.write(handshakePkt);
        this.msgsSent++;
        // Don't wait for handshake response — gateway doesn't send one
        this.handshakeDone = true;
        this.eventCallback({
          event: "connected",
          id: this.id,
          uid: this.uid,
        });
        resolve();
      });

      this.socket.on("data", (data) => {
        this.decoder.feed(data);
        try {
          const packets = this.decoder.decodeAll();
          for (const pkt of packets) {
            this.msgsReceived++;
            this.eventCallback({
              event: "message",
              id: this.id,
              msgId: pkt.msgId,
              data: pkt.data.toString("utf8"),
            });
          }
        } catch (e) {
          this.eventCallback({
            event: "error",
            id: this.id,
            message: e.message,
          });
          this.disconnect("decode error");
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
          this.eventCallback({
            event: "disconnected",
            id: this.id,
            reason: "connection closed",
          });
        }
      });
    });
  }

  send(msgId, data) {
    if (!this.connected || !this.socket) {
      return false;
    }
    try {
      const pkt = encodePacket(msgId, Buffer.from(data, "utf8"), this.cipher);
      this.socket.write(pkt);
      this.msgsSent++;
      return true;
    } catch (e) {
      this.eventCallback({
        event: "error",
        id: this.id,
        message: "send error: " + e.message,
      });
      return false;
    }
  }

  disconnect(reason) {
    if (this.socket) {
      this.connected = false;
      this.socket.destroy();
      this.socket = null;
      this.eventCallback({
        event: "disconnected",
        id: this.id,
        reason: reason || "manual disconnect",
      });
    }
  }
}

// ── WebSocket Client Session ───────────────────────────────────────
class ClientSession {
  constructor(ws, aesKey) {
    this.ws = ws;
    this.cipher = createCipher(aesKey);
    this.connections = new Map(); // id -> GatewayConnection
    this.autoSendTimer = null;
    this.totalSent = 0;
    this.totalReceived = 0;
    this.totalErrors = 0;
    this.uidCounter = 10000;
  }

  send(obj) {
    if (this.ws.readyState === 1) {
      this.ws.send(JSON.stringify(obj));
    }
  }

  async handleAction(msg) {
    switch (msg.action) {
      case "connect": {
        const id = msg.id || `conn-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
        const uid = msg.uid || ++this.uidCounter;
        const token = msg.token || "test_token_123";
        const host = msg.host || "127.0.0.1";
        const port = msg.port || 7888;
        const conn = new GatewayConnection(
          id, uid, token, host, port, this.cipher,
          (evt) => {
            if (evt.event === "message") this.totalReceived++;
            if (evt.event === "error") this.totalErrors++;
            this.send(evt);
          }
        );
        this.connections.set(id, conn);
        try {
          await conn.connect();
        } catch (e) {
          this.connections.delete(id);
          this.totalErrors++;
          this.send({ event: "connectFailed", id, reason: e.message });
        }
        break;
      }

      case "connectBulk": {
        const count = Math.min(msg.count || 10, 10000);
        const uidStart = msg.uidStart || (this.uidCounter + 1);
        const token = msg.token || "test_token_123";
        const host = msg.host || "127.0.0.1";
        const port = msg.port || 7888;
        const delayMs = msg.delayMs || 0;
        const batchSize = msg.batchSize || 200;
        let success = 0;
        let failed = 0;
        this.send({ event: "bulkProgress", total: count, success, failed });
        let lastProgressSent = 0;

        const allIds = [];
        for (let i = 0; i < count; i++) {
          const uid = uidStart + i;
          const id = `conn-${uid}`;
          if (!this.connections.has(id)) {
            allIds.push({ uid, id });
          } else {
            failed++;
          }
        }

        // Process in parallel batches
        for (let batchStart = 0; batchStart < allIds.length; batchStart += batchSize) {
          const batch = allIds.slice(batchStart, batchStart + batchSize);
          const promises = batch.map(({ uid, id }) => {
            const conn = new GatewayConnection(
              id, uid, token, host, port, this.cipher,
              (evt) => {
                if (evt.event === "message") this.totalReceived++;
                if (evt.event === "error") this.totalErrors++;
                // Only forward errors and messages for small connection counts
                // For large counts, the browser can't handle per-connection events
                if (this.connections.size <= 100) {
                  this.send(evt);
                } else if (evt.event === "error") {
                  this.send(evt);
                }
              }
            );
            this.connections.set(id, conn);
            return conn.connect().then(() => { success++; }, () => {
              this.connections.delete(id);
              this.totalErrors++;
              failed++;
            });
          });

          await Promise.all(promises);

          // Throttle progress updates: send at most every 5% or every batch
          const progressStep = Math.max(Math.floor(count / 20), 1);
          if (success + failed - lastProgressSent >= progressStep || batchStart + batchSize >= allIds.length) {
            this.send({ event: "bulkProgress", total: count, success, failed });
            lastProgressSent = success + failed;
          }

          if (delayMs > 0 && batchStart + batchSize < allIds.length) {
            await new Promise((r) => setTimeout(r, delayMs));
          }
        }
        this.send({ event: "bulkDone", total: count, success, failed });
        break;
      }

      case "send": {
        const conn = this.connections.get(msg.id);
        if (conn) {
          const ok = conn.send(msg.msgId || 1, msg.data || "ping");
          if (ok) this.totalSent++;
        }
        break;
      }

      case "broadcast": {
        const msgId = msg.msgId || 1;
        const data = msg.data || "ping";
        for (const conn of this.connections.values()) {
          if (conn.send(msgId, data)) this.totalSent++;
        }
        break;
      }

      case "autoSend": {
        if (this.autoSendTimer) {
          clearInterval(this.autoSendTimer);
        }
        const interval = msg.intervalMs || 1000;
        const msgId = msg.msgId || 1;
        const data = msg.data || "auto-ping";
        this.autoSendTimer = setInterval(() => {
          for (const conn of this.connections.values()) {
            if (conn.send(msgId, data)) this.totalSent++;
          }
        }, interval);
        this.send({ event: "autoSendStarted", intervalMs: interval });
        break;
      }

      case "stopAutoSend": {
        if (this.autoSendTimer) {
          clearInterval(this.autoSendTimer);
          this.autoSendTimer = null;
        }
        this.send({ event: "autoSendStopped" });
        break;
      }

      case "disconnect": {
        const conn = this.connections.get(msg.id);
        if (conn) {
          conn.disconnect("manual");
          this.connections.delete(msg.id);
        }
        break;
      }

      case "disconnectAll": {
        for (const conn of this.connections.values()) {
          conn.disconnect("disconnectAll");
        }
        this.connections.clear();
        if (this.autoSendTimer) {
          clearInterval(this.autoSendTimer);
          this.autoSendTimer = null;
        }
        this.send({ event: "allDisconnected" });
        break;
      }

      case "getStats": {
        this.sendStats();
        break;
      }
    }
  }

  sendStats() {
    let active = 0;
    let totalSent = 0;
    let totalReceived = 0;
    for (const conn of this.connections.values()) {
      if (conn.connected) active++;
      totalSent += conn.msgsSent;
      totalReceived += conn.msgsReceived;
    }
    this.send({
      event: "stats",
      activeConnections: active,
      totalConnections: this.connections.size,
      totalSent: totalSent,
      totalReceived: totalReceived,
      totalErrors: this.totalErrors,
    });
  }

  cleanup() {
    if (this.autoSendTimer) {
      clearInterval(this.autoSendTimer);
    }
    for (const conn of this.connections.values()) {
      conn.disconnect("session closed");
    }
    this.connections.clear();
  }
}

// ── HTTP Server (serves web client) ────────────────────────────────
const PROXY_PORT = process.env.PROXY_PORT || 3000;
const GATEWAY_HOST = process.env.GATEWAY_HOST || "127.0.0.1";
const GATEWAY_PORT = parseInt(process.env.GATEWAY_PORT || "7888", 10);
const AES_KEY = process.env.AES_KEY || DEFAULT_AES_KEY;

const httpServer = http.createServer((req, res) => {
  let filePath;
  let contentType = "text/html; charset=utf-8";

  if (req.url === "/" || req.url === "/index.html") {
    filePath = path.join(__dirname, "index.html");
  } else if (req.url === "/game" || req.url === "/game.html") {
    filePath = path.join(__dirname, "game.html");
  } else if (req.url === "/config") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({
      gatewayHost: GATEWAY_HOST,
      gatewayPort: GATEWAY_PORT,
      proxyPort: PROXY_PORT,
    }));
    return;
  } else {
    res.writeHead(404);
    res.end("Not Found");
    return;
  }

  fs.readFile(filePath, (err, data) => {
    if (err) {
      res.writeHead(500);
      res.end("Internal Server Error");
      return;
    }
    res.writeHead(200, { "Content-Type": contentType });
    res.end(data);
  });
});

// ── WebSocket Server ───────────────────────────────────────────────
const wss = new WebSocketServer({ server: httpServer, path: "/ws" });

wss.on("connection", (ws) => {
  const session = new ClientSession(ws, AES_KEY);
  console.log(`[Proxy] New WebSocket client connected`);

  // Send welcome with config
  session.send({
    event: "ready",
    config: {
      gatewayHost: GATEWAY_HOST,
      gatewayPort: GATEWAY_PORT,
    },
  });

  ws.on("message", (raw) => {
    try {
      const msg = JSON.parse(raw.toString());
      session.handleAction(msg).catch((e) => {
        session.send({ event: "error", message: e.message });
      });
    } catch (e) {
      session.send({ event: "error", message: "invalid JSON: " + e.message });
    }
  });

  ws.on("close", () => {
    console.log(`[Proxy] WebSocket client disconnected, cleaning up ${session.connections.size} TCP connections`);
    session.cleanup();
  });

  ws.on("error", () => {
    session.cleanup();
  });

  // Send stats every 2 seconds (or 5 seconds for large connection counts)
  const statsInterval = 2000;
  const statsTimer = setInterval(() => {
    if (ws.readyState === 1) {
      session.sendStats();
    } else {
      clearInterval(statsTimer);
    }
  }, statsInterval);
});

// ── Start ──────────────────────────────────────────────────────────
httpServer.listen(PROXY_PORT, () => {
  console.log("═══════════════════════════════════════════");
  console.log("  Rust MMO Gate - WebSocket Proxy");
  console.log("═══════════════════════════════════════════");
  console.log(`  Web Client:  http://localhost:${PROXY_PORT}`);
  console.log(`  WebSocket:   ws://localhost:${PROXY_PORT}/ws`);
  console.log(`  Gateway TCP: ${GATEWAY_HOST}:${GATEWAY_PORT}`);
  console.log(`  AES Key:     ${AES_KEY.substring(0, 16)}...`);
  console.log("═══════════════════════════════════════════");
  console.log("");
  console.log("Open http://localhost:" + PROXY_PORT + " in your browser to start testing.");
  console.log("Open multiple tabs for more connections!");
});
