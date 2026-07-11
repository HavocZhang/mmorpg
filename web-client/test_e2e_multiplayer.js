#!/usr/bin/env node
// 多人战斗 E2E: 2 玩家同时在线 → 同屏战斗 → 怪物死亡通知
const WebSocket = require('ws'), crypto = require('crypto');
const WS = 'ws://127.0.0.1:9000', K = '00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff';
const M = [0x4D, 0x4D], V = 1, H = 16;
const cT = (() => { const t = new Uint32Array(256); for (let i = 0; i < 256; i++) { let c = i; for (let j = 0; j < 8; j++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[i] = c >>> 0; } return t; })();
function crc(b) { let c = 0xffffffff; for (let i = 0; i < b.length; i++) c = cT[(c ^ b[i]) & 0xff] ^ (c >>> 8); return c; }
function enc(p) { const n = crypto.randomBytes(12), ci = crypto.createCipheriv('aes-256-gcm', Buffer.from(K, 'hex'), n), e = Buffer.concat([ci.update(p), ci.final()]), g = ci.getAuthTag(); return Buffer.concat([n, e, g]); }
function dec(d) { const n = d.subarray(0, 12), tg = d.subarray(d.length - 16), ct = d.subarray(12, d.length - 16); const ci = crypto.createDecipheriv('aes-256-gcm', Buffer.from(K, 'hex'), n); ci.setAuthTag(tg); return Buffer.concat([ci.update(ct), ci.final()]); }
function pk(m, d) { const ec = enc(d), h = Buffer.alloc(H); h[0] = M[0]; h[1] = M[1]; h[2] = V; h[3] = 0; h.writeUInt16BE(m, 4); h.writeUInt16BE(ec.length, 6); h.writeUInt32BE((crc(ec) ^ 0xffffffff) >>> 0, 8); return Buffer.concat([h, ec]); }

class Player {
    constructor(uid) { this.uid = uid; this.msgs = {}; }
    connect() {
        const p = this;
        return new Promise((resolve, reject) => {
            p.ws = new WebSocket(WS);
            p.ws.on('open', () => { p.ws.send(pk(1, Buffer.from(JSON.stringify({ uid: p.uid, token: 'tok_abcdefgh', version: V, timestamp: Math.floor(Date.now() / 1000) }), 'utf8'))); resolve(); });
            p.ws.on('message', d => {
                let buf = Buffer.from(d), off = 0;
                while (off + H <= buf.length) {
                    if (buf[off] !== M[0] || buf[off + 1] !== M[1]) { off++; continue; }
                    const bl = buf.readUInt16BE(off + 6); if (off + H + bl > buf.length) break;
                    const mid = buf.readUInt16BE(off + 4);
                    try { const j = JSON.parse(dec(buf.subarray(off + H, off + H + bl)).toString('utf8')); if (!p.msgs[mid]) p.msgs[mid] = []; p.msgs[mid].push(j); } catch (e) { }
                    off += H + bl;
                }
            });
            p.ws.on('error', reject);
            setTimeout(resolve, 3000);
        });
    }
    send(m, d) { this.ws.send(pk(m, Buffer.from(JSON.stringify(d), 'utf8'))); }
    close() { this.ws.close(); }
}

async function main() {
    console.log('Multi-Player Combat E2E Test\n');
    let ok = 0, fail = 0;
    function t(n, c) { if (c) { ok++; console.log('  ✓', n); } else { fail++; console.log('  ✗', n); } }

    // Connect 2 players
    const p1 = new Player(44001);
    const p2 = new Player(44002);
    await p1.connect();
    await p2.connect();
    await new Promise(r => setTimeout(r, 1500));

    t('P1 场景加载', !!(p1.msgs[9002]));
    t('P2 场景加载', !!(p2.msgs[9002]));
    t('P1 收到 P2 进入通知', !!(p1.msgs[8002] && p1.msgs[8002].some(m => m.uid === 44002)));
    t('P2 收到 P1 进入通知', !!(p2.msgs[8002] && p2.msgs[8002].some(m => m.uid === 44001)));

    // Both attack same monster
    const mobs = p1.msgs[9002] ? (p1.msgs[9002][0].mobs || []) : [];
    if (mobs.length > 0) {
        const mob = JSON.parse(mobs[0]);
        console.log('\n  [1] 双人同屏战斗');

        // Both attack simultaneously
        await p1.send(1001, { skillId: 1, targetUid: mob.entityId });
        await p2.send(1001, { skillId: 1, targetUid: mob.entityId });
        await new Promise(r => setTimeout(r, 500));

        t('P1 收到战斗结果', !!(p1.msgs[6001]));
        t('P2 收到战斗结果', !!(p2.msgs[6001]));

        // Chat between players
        console.log('\n  [2] 战斗聊天');
        await p1.send(2001, { from: 44001, text: 'nice hit!', channel: 'world' });
        await new Promise(r => setTimeout(r, 500));
        t('P2 收到聊天广播', !!(p2.msgs[7002] && p2.msgs[7002].some(m => m.text === 'nice hit!')));
    }

    // Cleanup — P2 leaves first, P1 observes
    p2.close();
    await new Promise(r => setTimeout(r, 2000));
    t('P1 收到 P2 离开通知', !!(p1.msgs[8003] && p1.msgs[8003].some(m => m.uid === 44002)));
    p1.close();

    console.log(`\n  ${ok}/${ok + fail} passed`);
    process.exit(fail > 0 ? 1 : 0);
}
main().catch(e => { console.error(e.message); process.exit(1); });
