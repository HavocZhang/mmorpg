#!/usr/bin/env node
// 持久化 E2E: 玩家上线 → 打怪升级 → 下线 → 再上线验证装备/等级保留
const WebSocket = require('ws'), crypto = require('crypto');
const WS = 'ws://127.0.0.1:9000', K = '00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff';
const M = [0x4D, 0x4D], V = 1, H = 16;
const cT = (() => { const t = new Uint32Array(256); for (let i = 0; i < 256; i++) { let c = i; for (let j = 0; j < 8; j++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[i] = c >>> 0; } return t; })();
function crc(b) { let c = 0xffffffff; for (let i = 0; i < b.length; i++) c = cT[(c ^ b[i]) & 0xff] ^ (c >>> 8); return c; }
function enc(p) { const n = crypto.randomBytes(12), ci = crypto.createCipheriv('aes-256-gcm', Buffer.from(K, 'hex'), n), e = Buffer.concat([ci.update(p), ci.final()]), g = ci.getAuthTag(); return Buffer.concat([n, e, g]); }
function dec(d) { const n = d.subarray(0, 12), tg = d.subarray(d.length - 16), ct = d.subarray(12, d.length - 16); const ci = crypto.createDecipheriv('aes-256-gcm', Buffer.from(K, 'hex'), n); ci.setAuthTag(tg); return Buffer.concat([ci.update(ct), ci.final()]); }
function pk(m, d) { const ec = enc(d), h = Buffer.alloc(H); h[0] = M[0]; h[1] = M[1]; h[2] = V; h[3] = 0; h.writeUInt16BE(m, 4); h.writeUInt16BE(ec.length, 6); h.writeUInt32BE((crc(ec) ^ 0xffffffff) >>> 0, 8); return Buffer.concat([h, ec]); }

async function connect(uid) {
    return new Promise((resolve, reject) => {
        const ws = new WebSocket(WS); const msgs = {};
        ws.on('open', () => { ws.send(pk(1, Buffer.from(JSON.stringify({ uid, token: 'tok_abcdefgh', version: V, timestamp: Math.floor(Date.now() / 1000) }), 'utf8'))); resolve({ ws, msgs }); });
        ws.on('message', d => {
            let buf = Buffer.from(d), off = 0;
            while (off + H <= buf.length) {
                if (buf[off] !== M[0] || buf[off + 1] !== M[1]) { off++; continue; }
                const bl = buf.readUInt16BE(off + 6); if (off + H + bl > buf.length) break;
                const mid = buf.readUInt16BE(off + 4);
                try { const j = JSON.parse(dec(buf.subarray(off + H, off + H + bl)).toString('utf8')); if (!msgs[mid]) msgs[mid] = []; msgs[mid].push(j); } catch (e) { }
                off += H + bl;
            }
        });
        ws.on('error', reject);
        setTimeout(() => resolve({ ws, msgs }), 3000);
    });
}

async function main() {
    console.log('Persistence E2E Test\n');
    let ok = 0, fail = 0;
    function t(n, c) { if (c) { ok++; console.log('  ✓', n); } else { fail++; console.log('  ✗', n); } }

    const TEST_UID = 99991;

    // === Session 1: Play, get items ===
    console.log('[1] 第一次上线');
    let c = await connect(TEST_UID);
    await new Promise(r => setTimeout(r, 1500));

    const attrs1 = c.msgs[5006] || c.msgs[5001];
    t('首次登录收到属性', !!attrs1);
    const initialLevel = attrs1 ? (attrs1[0].level || 1) : 1;

    // Attack a monster to gain exp
    const mobs = c.msgs[9002] ? (c.msgs[9002][0].mobs || []) : [];
    if (mobs.length > 0) {
        const mob = JSON.parse(mobs[0]);
        c.ws.send(pk(1001, Buffer.from(JSON.stringify({ skillId: 1, targetUid: mob.entityId }), 'utf8')));
        await new Promise(r => setTimeout(r, 500));
        t('首次战斗完成', !!(c.msgs[6001]));
    }

    // Disconnect
    c.ws.close();
    await new Promise(r => setTimeout(r, 2000));
    console.log('[2] 下线');

    // === Session 2: Reconnect, verify state ===
    console.log('[3] 第二次上线 (应保留数据)');
    c = await connect(TEST_UID);
    await new Promise(r => setTimeout(r, 2000));

    const attrs2 = c.msgs[5006] || c.msgs[5001];
    t('重新登录收到属性', !!attrs2);

    if (attrs2 && attrs2[0]) {
        const lv = attrs2[0].level;
        t('等级 >= 首次等级', lv >= initialLevel);
    }

    const inv2 = c.msgs[5003];
    t('背包数据保留', !!(inv2 && inv2.length > 0));

    c.ws.close();
    console.log(`\n  ${ok}/${ok + fail} passed`);
    process.exit(fail > 0 ? 1 : 0);
}
main().catch(e => { console.error(e.message); process.exit(1); });
