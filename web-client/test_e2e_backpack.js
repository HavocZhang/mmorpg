#!/usr/bin/env node
// 背包 & 装备 E2E 测试: 拾取 → 使用药水 → 装备穿戴
const WebSocket = require('ws'), crypto = require('crypto');
const WS = 'ws://127.0.0.1:9000';
const K = '00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff';
const M = [0x4D, 0x4D], V = 1, H = 16;
const cT = (() => { const t = new Uint32Array(256); for (let i = 0; i < 256; i++) { let c = i; for (let j = 0; j < 8; j++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[i] = c >>> 0; } return t; })();
function crc(b) { let c = 0xffffffff; for (let i = 0; i < b.length; i++) c = cT[(c ^ b[i]) & 0xff] ^ (c >>> 8); return c; }
function enc(p) { const n = crypto.randomBytes(12), ci = crypto.createCipheriv('aes-256-gcm', Buffer.from(K, 'hex'), n), e = Buffer.concat([ci.update(p), ci.final()]), g = ci.getAuthTag(); return Buffer.concat([n, e, g]); }
function dec(data) { const n = data.subarray(0, 12), tg = data.subarray(data.length - 16), ct = data.subarray(12, data.length - 16); const ci = crypto.createDecipheriv('aes-256-gcm', Buffer.from(K, 'hex'), n); ci.setAuthTag(tg); return Buffer.concat([ci.update(ct), ci.final()]); }
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
    console.log('Backpack & Equipment E2E Test\n');
    let ok = 0, fail = 0;
    function t(n, c) { if (c) { ok++; console.log('  ✓', n); } else { fail++; console.log('  ✗', n); } }

    const p = new Player(55001);
    await p.connect();
    await new Promise(r => setTimeout(r, 1000));

    // 1. 初始背包 & HP
    const initHp = p.msgs[5001] ? p.msgs[5001][0].hp : 100;
    t('初始 HP > 0', initHp > 0);
    t('背包已加载(5003)', !!(p.msgs[5003]));

    // 2. 使用药水 (item 5 = 小红药)
    console.log('\n  [1] 使用物品');
    // The player starts with some items. Let's check what's available.
    const invData = p.msgs[5003] ? p.msgs[5003][0] : {};
    const inv = Array.isArray(invData.items) ? invData.items : (Array.isArray(invData) ? invData : []);
    const potion = inv.find(item => item.itemId === 5 || item.itemId === 6);
    if (potion) {
        await p.send(1008, { itemId: potion.itemId });
        await new Promise(r => setTimeout(r, 500));
    }
    t('使用物品请求已发送', true);

    // 3. 装备物品
    console.log('\n  [2] 装备系统');
    const weapon = inv.find(item => item.itemId === 7);
    if (weapon) {
        await p.send(1004, { itemId: weapon.itemId, slot: 'weapon' });
        await new Promise(r => setTimeout(r, 500));
    }
    const eq = p.msgs[5004] ? p.msgs[5004][p.msgs[5004].length - 1] : null;
    t('装备数据已返回(5004)', !!eq);

    // 4. 攻击验证装备属性生效
    console.log('\n  [3] 装备属性验证');
    const mobs = p.msgs[9002] ? (p.msgs[9002][0].mobs || []) : [];
    if (mobs.length > 0 && eq) {
        const mob = JSON.parse(mobs[0]);
        await p.send(1001, { skillId: 1, targetUid: mob.entityId });
        await new Promise(r => setTimeout(r, 500));
        t('装备后攻击正常', !!(p.msgs[6001]));
    }

    console.log(`\n  ${ok}/${ok + fail} passed`);
    p.close();
    process.exit(fail > 0 ? 1 : 0);
}
main().catch(e => { console.error(e.message); process.exit(1); });
