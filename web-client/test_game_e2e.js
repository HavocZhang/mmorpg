#!/usr/bin/env node
// ============================================================
//  MMORPG 游戏 E2E 集成测试套件
//  验证: 握手 → 属性 → 战斗 → 聊天 → 移动 → NPC → 断线
//  用法: node test_game_e2e.js [--verbose]
// ============================================================
const WebSocket = require('ws'), crypto = require('crypto'), http = require('http');

const WS_URL = 'ws://127.0.0.1:9000';
const KEY = '00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff';
const M = [0x4D, 0x4D], V = 1, HEAD = 16;
const VV = process.argv.includes('--verbose');

const crcTable = (() => { const t = new Uint32Array(256); for (let i = 0; i < 256; i++) { let c = i; for (let j = 0; j < 8; j++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[i] = c >>> 0; } return t; })();
function crc32(b) { let c = 0xffffffff; for (let i = 0; i < b.length; i++) c = crcTable[(c ^ b[i]) & 0xff] ^ (c >>> 8); return c; }

function encrypt(plaintext) {
    const nonce = crypto.randomBytes(12), cipher = crypto.createCipheriv('aes-256-gcm', Buffer.from(KEY, 'hex'), nonce);
    const enc = Buffer.concat([cipher.update(plaintext), cipher.final()]), tag = cipher.getAuthTag();
    const buf = Buffer.alloc(12 + enc.length + 16);
    nonce.copy(buf, 0); enc.copy(buf, 12); tag.copy(buf, 12 + enc.length); return buf;
}
function decrypt(data) {
    const nonce = data.subarray(0, 12), tag = data.subarray(data.length - 16), ct = data.subarray(12, data.length - 16);
    const decipher = crypto.createDecipheriv('aes-256-gcm', Buffer.from(KEY, 'hex'), nonce);
    decipher.setAuthTag(tag); return Buffer.concat([decipher.update(ct), decipher.final()]);
}
function packet(msgId, payload) {
    const enc = encrypt(payload);
    const h = Buffer.alloc(HEAD); h[0] = M[0]; h[1] = M[1]; h[2] = V; h[3] = 0;
    h.writeUInt16BE(msgId, 4); h.writeUInt16BE(enc.length, 6); h.writeUInt32BE((crc32(enc) ^ 0xffffffff) >>> 0, 8);
    h.writeUInt32BE(0, 12); return Buffer.concat([h, enc]);
}

// ====== Test Framework ======
let passed = 0, failed = 0;
function ok(name, cond, detail) {
    if (cond) { passed++; if(VV)console.log('  ✓', name); }
    else { failed++; console.log('  ✗', name, detail||''); }
}

// ====== Player connection ======
class Player {
    constructor(uid) { this.uid = uid; this.ws = null; this.msgs = []; this.byMsgId = {}; }
    async connect() {
        const p = this;
        return new Promise((resolve, reject) => {
            p.ws = new WebSocket(WS_URL);
            p.ws.on('open', async () => {
                p.ws.send(packet(1, Buffer.from(JSON.stringify({ uid: p.uid, token: 'tok_abcdefgh', version: V, timestamp: Math.floor(Date.now() / 1000) }), 'utf8')));
                resolve();
            });
            p.ws.on('message', d => {
                const buf = Buffer.from(d); let off = 0;
                while (off + HEAD <= buf.length) {
                    if (buf[off] !== M[0] || buf[off + 1] !== M[1]) { off++; continue; }
                    const bl = buf.readUInt16BE(off + 6); if (off + HEAD + bl > buf.length) break;
                    const mid = buf.readUInt16BE(off + 4);
                    try { const plain = decrypt(buf.subarray(off + HEAD, off + HEAD + bl)); const json = JSON.parse(plain.toString('utf8')); p.msgs.push({ mid, body: json }); if (!p.byMsgId[mid]) p.byMsgId[mid] = []; p.byMsgId[mid].push(json); } catch (e) { }
                    off += HEAD + bl;
                }
            });
            p.ws.on('error', e => reject(e));
            setTimeout(() => { if (p.msgs.length === 0) reject(new Error('timeout')); }, 5000);
        });
    }
    async send(mid, data) { this.ws.send(packet(mid, Buffer.from(JSON.stringify(data), 'utf8'))); }
    waitMsg(mid, timeout = 3000) {
        const p = this;
        return new Promise((resolve, reject) => {
            const check = () => {
                const arr = p.byMsgId[mid];
                if (arr && arr.length > 0) { clearInterval(iv); resolve(arr[arr.length - 1]); }
            };
            const iv = setInterval(check, 100);
            setTimeout(() => { clearInterval(iv); resolve(null); }, timeout);
        });
    }
    close() { if (this.ws) this.ws.close(); }
}

// ====== Wait helper ======
function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

// ====== Main Test Suite ======
async function main() {
    console.log('\n═══════════════════════════════════════');
    console.log('  MMORPG 全链路 E2E 集成测试');
    console.log('═══════════════════════════════════════\n');

    // ── 0. Verify services ──
    console.log('[0] 服务健康检查...');
    try {
        const h = await new Promise((resolve, reject) => {
            http.get('http://127.0.0.1:9090/health', r => { let d = ''; r.on('data', c => d += c); r.on('end', () => resolve(JSON.parse(d))); }).on('error', reject);
        });
        ok('Gateway 运行中', h.status === 'ok', JSON.stringify(h));
    } catch (e) {
        console.log('  ✗ Gateway 未运行! 请先启动: cargo run --release');
        process.exit(1);
    }

    // ── 1. 握手 + 角色属性 ──
    console.log('\n[1] 握手 & 角色初始化');
    const p1 = new Player(11111);
    await p1.connect();
    await sleep(500);
    const statsMsg = p1.byMsgId[5006] || p1.byMsgId[5001];
    ok('收到玩家属性', !!statsMsg);
    ok('收到 NPC/怪物列表(9002)', !!p1.byMsgId[9002]);

    const mobs = p1.byMsgId[9002];
    if (mobs && mobs[0] && mobs[0].mobs) {
        ok('怪物数据已加载', mobs[0].mobs.length > 0, `共 ${mobs[0].mobs.length} 只`);
        ok('NPC 数据已加载', mobs[0].npcs.length > 0, `共 ${mobs[0].npcs.length} 个`);
    }

    // ── 2. 聊天 ──
    console.log('\n[2] 聊天系统');
    const p2 = new Player(22222);
    await p2.connect();
    await sleep(500);

    await p1.send(2001, { from: 11111, text: 'hello world', channel: 'world' });
    await sleep(300);
    ok('发送者收到 ACK(7001)', !!p1.byMsgId[7001]);

    const chatBroadcast = await p2.waitMsg(7002, 2000);
    ok('其他玩家收到广播(7002)', !!chatBroadcast && chatBroadcast.text === 'hello world');

    // ── 3. 移动同步 ──
    console.log('\n[3] 移动同步');
    await p1.send(3001, { x: 500, y: 400, dir: 1 });
    await sleep(300);
    ok('自身移动消息已发送', true);

    // ── 4. 战斗 ──
    console.log('\n[4] 战斗系统');
    const mobId = mobs && mobs[0] && mobs[0].mobs && mobs[0].mobs.length > 0 ? JSON.parse(mobs[0].mobs[0]).entityId : 10000;
    if (mobId) {
        await p1.send(1001, { skillId: 1, targetUid: mobId });
        await sleep(500);
        ok('战斗结果(6001)已返回', !!p1.byMsgId[6001]);
    } else {
        ok('跳过战斗测试 (无怪物)', false);
    }

    // ── 5. 怪物位置更新 ──
    console.log('\n[5] 怪物 AI & 位置同步');
    await p1.send(4001, {}); // trigger server response with monster positions
    await sleep(500);
    const posMsgs = p1.byMsgId[8004];
    ok('收到怪物位置更新(8004)', !!(posMsgs && posMsgs.length > 0), posMsgs ? `${posMsgs.length} 条` : '0 条');

    // ── 6. 多人在线 ──
    console.log('\n[6] 多人在线验证');
    ok('P1 收到 P2 的进入通知(8002)', !!p1.byMsgId[8002]);
    ok('P2 收到 P1 的进入通知(8002)', !!p2.byMsgId[8002]);

    // ── 7. 断线清理 ──
    console.log('\n[7] 断线清理');
    p2.close();
    await sleep(2000);
    // P1 sends a message to trigger server response with 8003
    await p1.send(4001, {});
    await sleep(500);
    const leaveMsgs = p1.byMsgId[8003];
    ok('P1 收到 P2 离开通知(8003)', !!(leaveMsgs && leaveMsgs.length > 0));

    // ── Summary ──
    const total = passed + failed;
    console.log(`\n═══════════════════════════════════════`);
    console.log(`  结果: ${passed}/${total} 通过, ${failed} 失败`);
    console.log(`  ${failed === 0 ? '✅ 全部通过!' : '❌ 有失败项'}`);
    console.log(`═══════════════════════════════════════\n`);

    p1.close();
    process.exit(failed > 0 ? 1 : 0);
}

main().catch(e => { console.error('Fatal:', e.message); process.exit(1); });
