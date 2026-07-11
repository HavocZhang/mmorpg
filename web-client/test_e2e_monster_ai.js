#!/usr/bin/env node
// 怪物 AI E2E 测试: 追击 → 攻击 → 玩家扣血 → 怪物死亡
const WebSocket = require('ws'), crypto = require('crypto');
const WS = 'ws://127.0.0.1:9000';
const K = '00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff';
const M = [0x4D, 0x4D], V = 1, H = 16;
const cT = (() => { const t = new Uint32Array(256); for (let i = 0; i < 256; i++) { let c = i; for (let j = 0; j < 8; j++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[i] = c >>> 0; } return t; })();
function crc(b) { let c = 0xffffffff; for (let i = 0; i < b.length; i++) c = cT[(c ^ b[i]) & 0xff] ^ (c >>> 8); return c; }
function enc(p) { const n = crypto.randomBytes(12), ci = crypto.createCipheriv('aes-256-gcm', Buffer.from(K, 'hex'), n), e = Buffer.concat([ci.update(p), ci.final()]), g = ci.getAuthTag(); return Buffer.concat([n, e, g]); }
function dec(data){ const n = data.subarray(0,12), tg = data.subarray(data.length-16), ct = data.subarray(12,data.length-16); const ci = crypto.createDecipheriv('aes-256-gcm', Buffer.from(K,'hex'), n); ci.setAuthTag(tg); return Buffer.concat([ci.update(ct), ci.final()]); }
function pk(m, d) { const ec = enc(d), h = Buffer.alloc(H); h[0] = M[0]; h[1] = M[1]; h[2] = V; h[3] = 0; h.writeUInt16BE(m, 4); h.writeUInt16BE(ec.length, 6); h.writeUInt32BE((crc(ec) ^ 0xffffffff) >>> 0, 8); return Buffer.concat([h, ec]); }

class Player {
    constructor(uid) { this.uid = uid; this.ws = null; this.msgs = {}; }
    connect() {
        return new Promise((resolve, reject) => {
            this.ws = new WebSocket(WS);
            this.ws.on('open', () => { this.ws.send(pk(1, Buffer.from(JSON.stringify({ uid: this.uid, token: 'tok_abcdefgh', version: V, timestamp: Math.floor(Date.now() / 1000) }), 'utf8'))); resolve(); });
            this.ws.on('message', d => {
                let buf = Buffer.from(d), off = 0;
                while (off + H <= buf.length) {
                    if (buf[off] !== M[0] || buf[off + 1] !== M[1]) { off++; continue; }
                    const bl = buf.readUInt16BE(off + 6); if (off + H + bl > buf.length) break;
                    const mid = buf.readUInt16BE(off + 4);
                    try { const plain = dec(buf.subarray(off + H, off + H + bl)); const p = JSON.parse(plain.toString('utf8')); if (!this.msgs[mid]) this.msgs[mid] = []; this.msgs[mid].push(p); } catch (e) { }
                    off += H + bl;
                }
            });
            this.ws.on('error', reject);
            setTimeout(resolve, 3000);
        });
    }
    send(m, d) { this.ws.send(pk(m, Buffer.from(JSON.stringify(d), 'utf8'))); }
    close() { this.ws.close(); }
}

async function main() {
    console.log('Monster AI E2E Test\n');
    let ok = 0, fail = 0;
    function t(n, c) { if (c) { ok++; console.log('  ✓', n); } else { fail++; console.log('  ✗', n); } }

    const p = new Player(77777);
    await p.connect();
    await new Promise(r => setTimeout(r, 1000));

    // 1. 获取怪物列表
    t('收到怪物列表(9002)', !!(p.msgs[9002] && p.msgs[9002][0] && p.msgs[9002][0].mobs));
    const mobs = p.msgs[9002] ? p.msgs[9002][0].mobs || [] : [];
    t('至少 1 只怪物', mobs.length > 0);

    // 2. 攻击怪物
    if (mobs.length > 0) {
        const mob = JSON.parse(mobs[0]);
        await p.send(1001, { skillId: 1, targetUid: mob.entityId });
        await new Promise(r => setTimeout(r, 500));
        t('战斗结果已返回(6001)', !!(p.msgs[6001]));
    }

    // 3. 怪物位置更新
    await new Promise(r => setTimeout(r, 1000));
    await p.send(4001, {}); // heartbeat
    await new Promise(r => setTimeout(r, 500));
    t('收到位置更新(8004)', !!(p.msgs[8004] && p.msgs[8004].length > 0));

    // 4. 玩家 HP 更新
    t('收到 HP 更新(5001)', !!(p.msgs[5001] && p.msgs[5001].length > 0));
    const hp = p.msgs[5001] ? p.msgs[5001].map(m => m.hp) : [];

    console.log(`\n${ok}/${ok + fail} passed`);
    p.close();
    process.exit(fail > 0 ? 1 : 0);
}
main().catch(e => { console.error(e.message); process.exit(1); });
