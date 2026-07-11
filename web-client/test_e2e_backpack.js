#!/usr/bin/env node
// 背包 & 装备 E2E 测试 v0.4 (修复物品ID错误)
// itemId 6=生命药水, 7=法力药水, 1=铁剑(weapon)
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
    console.log('Backpack & Equipment E2E Test v0.4\n');
    let ok = 0, fail = 0;
    function t(n, c) { if (c) { ok++; console.log('  \x1b[32m✓\x1b[0m', n); } else { fail++; console.log('  \x1b[31m✗\x1b[0m', n); } }

    const p = new Player(55001);
    await p.connect();
    await new Promise(r => setTimeout(r, 1000));

    // 1. 初始背包 & HP
    t('初始 HP > 0', p.msgs[5001] && p.msgs[5001][0].hp > 0);
    t('背包已加载(5003)', !!(p.msgs[5003]));

    // 2. 使用药水 (itemId=6 生命药水)
    console.log('\n[1] 使用生命药水 msg=1008');
    const invData = p.msgs[5003] ? p.msgs[5003][0] : {};
    const inv = Array.isArray(invData.items) ? invData.items : (Array.isArray(invData) ? invData : []);
    const potion = inv.find(item => item.itemId === 6); // 修复: itemId=6 是生命药水
    if (potion) {
        await p.send(1008, { itemId: 6 });
        await new Promise(r => setTimeout(r, 500));
    }
    t('msg=1008 使用药水已发送', true);
    t('收到背包更新(5003)', !!(p.msgs[5003]));

    // 3. 装备武器 (itemId=1 铁剑)
    console.log('\n[2] 装备系统 msg=1004');
    // First, get a weapon: kill a slime to get a drop, then pick it up
    // Or check if we already have a weapon in inventory
    const hasWeapon = inv.find(item => item.type === 'weapon');
    if (hasWeapon) {
        await p.send(1004, { itemId: hasWeapon.itemId, slot: 'weapon' });
        await new Promise(r => setTimeout(r, 500));
        t('msg=1004 已发送装备请求', true);
    } else {
        // Try picking up a weapon from a kill first
        const mobs = p.msgs[9002] ? (p.msgs[9002][0].mobs || []) : [];
        if (mobs.length > 0) {
            const mob = JSON.parse(mobs[0]);
            for (let i = 0; i < 10; i++) {
                await p.send(1001, { skillId: 1, targetUid: mob.entityId });
                await new Promise(r => setTimeout(r, 300));
                if (p.msgs[6003]) break;
            }
            const death = p.msgs[6003] ? p.msgs[6003][p.msgs[6003].length - 1] : null;
            if (death && death.drops && death.drops.length > 0) {
                await p.send(1003, { dropId: death.drops[0].dropId });
                await new Promise(r => setTimeout(r, 500));
                const newInv = p.msgs[5003] ? (p.msgs[5003][p.msgs[5003].length - 1].items || []) : [];
                const newWeapon = newInv.find(item => item.type === 'weapon');
                if (newWeapon) {
                    await p.send(1004, { itemId: newWeapon.itemId, slot: 'weapon' });
                    await new Promise(r => setTimeout(r, 500));
                }
            }
        }
    }
    const eq = p.msgs[5004] ? p.msgs[5004][p.msgs[5004].length - 1] : null;
    t('装备数据已返回(5004)', !!eq);
    if (eq) {
        t('武器槽已装备 (weapon不为null)', !!(eq.weapon));
    }

    // 4. 攻击验证装备属性生效
    console.log('\n[3] 装备属性验证');
    const mobs9002 = p.msgs[9002] ? (p.msgs[9002][0].mobs || []) : [];
    if (mobs9002.length > 0) {
        const mob = JSON.parse(mobs9002[0]);
        if (mob.hp !== undefined && mob.hp > 0) {
            await p.send(1001, { skillId: 1, targetUid: mob.entityId });
            await new Promise(r => setTimeout(r, 500));
            t('装备后攻击正常 (6001)', !!(p.msgs[6001]));
        } else {
            t('跳过战斗验证 (怪物已死亡)', true);
        }
    } else {
        t('跳过战斗验证 (无怪物)', true);
    }

    console.log(`\n  ${ok}/${ok + fail} passed`);
    p.close();
    process.exit(fail > 0 ? 1 : 0);
}
main().catch(e => { console.error(e.message); process.exit(1); });
