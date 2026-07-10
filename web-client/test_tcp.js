/**
 * 直接 TCP 连接测试 - 绕过 WebSocket 代理
 * 验证网关是否正确发送下行消息
 */

const net = require("net");
const crypto = require("crypto");

const HOST = "127.0.0.1";
const PORT = 7888;
const AES_KEY = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

// CRC32
const crc32Table = (() => {
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let j = 0; j < 8; j++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[i] = c >>> 0;
  }
  return table;
})();

function crc32(buf) {
  let crc = 0xffffffff;
  for (let i = 0; i < buf.length; i++) crc = crc32Table[(crc ^ buf[i]) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

// AES-256-GCM
const key = Buffer.from(AES_KEY, "hex");

function encrypt(plaintext) {
  const nonce = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv("aes-256-gcm", key, nonce);
  const encrypted = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  const tag = cipher.getAuthTag();
  return Buffer.concat([nonce, encrypted, tag]);
}

function decrypt(data) {
  const nonce = data.subarray(0, 12);
  const tag = data.subarray(data.length - 16);
  const ciphertext = data.subarray(12, data.length - 16);
  const decipher = crypto.createDecipheriv("aes-256-gcm", key, nonce);
  decipher.setAuthTag(tag);
  return Buffer.concat([decipher.update(ciphertext), decipher.final()]);
}

// 编码包
function encodePacket(msgId, plaintext) {
  const encrypted = encrypt(plaintext);
  const header = Buffer.alloc(16);
  header[0] = 0x4d;
  header[1] = 0x4d;
  header[2] = 1; // version
  header[3] = 0;
  header.writeUInt16BE(msgId, 4);
  header.writeUInt16BE(encrypted.length, 6);
  header.writeUInt32BE(crc32(encrypted), 8);
  header.writeUInt32BE(0, 12);
  return Buffer.concat([header, encrypted]);
}

// 解码器
class Decoder {
  constructor() {
    this.buf = Buffer.alloc(0);
  }
  feed(data) {
    this.buf = Buffer.concat([this.buf, data]);
  }
  decodeAll() {
    const packets = [];
    while (this.buf.length >= 16) {
      if (this.buf[0] !== 0x4d || this.buf[1] !== 0x4d) {
        console.log("  [DECODER] Magic mismatch! Got:", this.buf[0], this.buf[1]);
        console.log("  [DECODER] First 32 bytes:", this.buf.subarray(0, 32).toString("hex"));
        throw new Error("magic mismatch");
      }
      const version = this.buf[2];
      const msgId = this.buf.readUInt16BE(4);
      const bodyLen = this.buf.readUInt16BE(6);
      const crc = this.buf.readUInt32BE(8);
      
      console.log(`  [DECODER] Header: magic=OK version=${version} msgId=${msgId} bodyLen=${bodyLen} crc=${crc}`);
      
      if (bodyLen > 8192) throw new Error(`body too large: ${bodyLen}`);
      const totalLen = 16 + bodyLen;
      if (this.buf.length < totalLen) {
        console.log(`  [DECODER] Need more data: have ${this.buf.length} need ${totalLen}`);
        break;
      }
      const body = this.buf.subarray(16, totalLen);
      if (crc32(body) !== crc) {
        console.log(`  [DECODER] CRC mismatch!`);
        throw new Error("CRC mismatch");
      }
      let plaintext;
      try {
        plaintext = decrypt(body);
        console.log(`  [DECODER] Decrypted: ${plaintext.toString("utf8").substring(0, 200)}`);
      } catch (e) {
        console.log(`  [DECODER] Decrypt failed: ${e.message}`);
        throw e;
      }
      packets.push({ msgId, data: plaintext.toString("utf8") });
      this.buf = this.buf.subarray(totalLen);
    }
    return packets;
  }
}

// 测试
console.log("\n=== 直接 TCP 连接测试 ===\n");

const socket = new net.Socket();
const decoder = new Decoder();
let messages = [];

socket.connect(PORT, HOST, () => {
  console.log("TCP connected, sending handshake...");
  
  // 发送握手
  const ts = Math.floor(Date.now() / 1000);
  const handshake = JSON.stringify({ uid: 20001, token: "test_token_123", version: 1, timestamp: ts });
  const pkt = encodePacket(0x0001, Buffer.from(handshake, "utf8"));
  socket.write(pkt);
  console.log("Handshake sent, waiting for messages...");
});

socket.on("data", (data) => {
  console.log(`\n[TCP] Received ${data.length} bytes`);
  console.log(`[TCP] Raw hex (first 64): ${data.subarray(0, 64).toString("hex")}`);
  
  decoder.feed(data);
  try {
    const packets = decoder.decodeAll();
    for (const pkt of packets) {
      console.log(`\n[MESSAGE] msgId=${pkt.msgId} data=${pkt.data.substring(0, 200)}`);
      messages.push(pkt);
    }
  } catch (e) {
    console.log(`[ERROR] Decode failed: ${e.message}`);
    socket.destroy();
  }
});

socket.on("error", (err) => {
  console.log(`[ERROR] Socket error: ${err.message}`);
});

socket.on("close", () => {
  console.log(`\n[CLOSE] Connection closed. Total messages received: ${messages.length}`);
  messages.forEach(m => console.log(`  msgId=${m.msgId}`));
});

// 5秒后发送聊天消息
setTimeout(() => {
  if (messages.length > 0) {
    console.log("\n[TEST] Sending chat message...");
    const chat = JSON.stringify({ text: "Hello from direct TCP!" });
    socket.write(encodePacket(2001, Buffer.from(chat, "utf8")));
  }
}, 5000);

// 10秒后关闭
setTimeout(() => {
  console.log("\n[TEST] Test complete, closing...");
  socket.destroy();
  process.exit(0);
}, 10000);
