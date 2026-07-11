#!/usr/bin/env node
// 组队 E2E: 邀请 → 接受 → 离开
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
    console.log('Party System E2E Test\n');
    let ok = 0, fail = 0;
    function t(n, c) { if (c) { ok++; console.log('  ✓', n); } else { fail++; console.log('  ✗', n); } }

    const p1 = await connect(91001);
    const p2 = await connect(91002);
    await new Promise(r => setTimeout(r, 1500));

    // 1. P1 invites P2 (msg 2002)
    console.log('[1] P1 邀请 P2');
    p1.ws.send(pk(2002, Buffer.from(JSON.stringify({ targetUid: 91002 }), 'utf8')));
    await new Promise(r => setTimeout(r, 500));
    t('P1 收到创建确认(7001)', !!(p1.msgs[7001] && p1.msgs[7001].some(m => m.type === 'party_created')));

    // 2. P2 receives invite
    t('P2 收到邀请(7002)', !!(p2.msgs[7002] && p2.msgs[7002].some(m => m.type === 'party_invite')));

    // 3. P2 accepts (msg 2003)
    console.log('[2] P2 接受邀请');
    p2.ws.send(pk(2003, Buffer.from(JSON.stringify({}), 'utf8')));
    await new Promise(r => setTimeout(r, 500));
    t('P2 收到加入确认(7001)', !!(p2.msgs[7001] && p2.msgs[7001].some(m => m.type === 'party_joined')));
    // P1 receives join notify via broadcast (may arrive with slight delay)
    await new Promise(r => setTimeout(r, 800));
    t('P1 收到 P2 加入通知(7002)', !!(p1.msgs[7002] && p1.msgs[7002].some(m => m.type === 'party_join' || m.type === 'party_invite')));

    // 4. P2 leaves (msg 2004)
    console.log('[3] P2 离开队伍');
    p2.ws.send(pk(2004, Buffer.from(JSON.stringify({}), 'utf8')));
    await new Promise(r => setTimeout(r, 500));
    t('P2 收到离开确认(7001)', !!(p2.msgs[7001] && p2.msgs[7001].some(m => m.type === 'party_left')));

    p1.ws.close(); p2.ws.close();
    console.log(`\n  ${ok}/${ok + fail} passed`);
    process.exit(fail > 0 ? 1 : 0);
}
main().catch(e => { console.error(e.message); process.exit(1); });
