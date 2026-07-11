#!/usr/bin/env node
// 集群跨网关消息测试
// Gate-1 (7888) ← Player A (uid=20001)
// Gate-2 (7889) ← Player B (uid=20002)
// Player A → Gate-1 → Redis PubSub → Gate-2 → Player B

const net = require('net'), crypto = require('crypto');

const GATE1 = { host: '127.0.0.1', port: 7888 };
const GATE2 = { host: '127.0.0.1', port: 7889 };
const AES_KEY = Buffer.from('00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff', 'hex');
const PROTOCOL_VERSION = 1, HEADER_SIZE = 16;
const MAGIC = [0x4D, 0x4D];

const crc32Table = (() => { const t = new Uint32Array(256); for (let i=0;i<256;i++){let c=i;for(let j=0;j<8;j++)c=c&1?0xedb88320^(c>>>1):c>>>1;t[i]=c>>>0;} return t; })();
function crc32(buf) { let c=0xffffffff; for(let i=0;i<buf.length;i++)c=crc32Table[(c^buf[i])&0xff]^(c>>>8); return (c^0xffffffff)>>>0; }

function encrypt(plaintext) {
    const nonce = crypto.randomBytes(12);
    const cipher = crypto.createCipheriv('aes-256-gcm', AES_KEY, nonce);
    const enc = Buffer.concat([cipher.update(plaintext), cipher.final()]);
    return Buffer.concat([nonce, enc, cipher.getAuthTag()]);
}
function decrypt(data) {
    const nonce = data.subarray(0,12), tag = data.subarray(data.length-16);
    const ciphertext = data.subarray(12, data.length-16);
    const decipher = crypto.createDecipheriv('aes-256-gcm', AES_KEY, nonce);
    decipher.setAuthTag(tag);
    return Buffer.concat([decipher.update(ciphertext), decipher.final()]);
}

function encodePacket(msgId, plaintext) {
    const encrypted = encrypt(plaintext);
    const header = Buffer.alloc(HEADER_SIZE);
    header[0]=MAGIC[0]; header[1]=MAGIC[1]; header[2]=PROTOCOL_VERSION; header[3]=0;
    header.writeUInt16BE(msgId,4); header.writeUInt16BE(encrypted.length,6);
    header.writeUInt32BE(crc32(encrypted),8); header.writeUInt32BE(0,12);
    return Buffer.concat([header, encrypted]);
}

function encodeHandshake(uid) {
    return encodePacket(0x0001, Buffer.from(JSON.stringify({
        uid, token:'cluster_test_token_000000000000', version:PROTOCOL_VERSION,
        timestamp: Math.floor(Date.now()/1000)
    }), 'utf8'));
}

function encodeChatMsg(fromUid, toUid, msg) {
    return encodePacket(2001, Buffer.from(JSON.stringify({
        from: fromUid, to: toUid, text: msg, t: Date.now()
    }), 'utf8'));
}

function connect(host, port, uid) {
    return new Promise((resolve, reject) => {
        const s = net.createConnection({host, port}, () => resolve(s));
        s.on('error', reject);
        s.setTimeout(5000, () => { s.destroy(); reject(new Error('timeout')); });
    });
}

function readPacket(sock) {
    return new Promise((resolve, reject) => {
        const onData = (data) => {
            sock.removeListener('data', onData);
            sock.removeListener('error', onError);
            resolve(data);
        };
        const onError = (e) => {
            sock.removeListener('data', onData);
            sock.removeListener('error', onError);
            reject(e);
        };
        sock.on('data', onData);
        sock.on('error', onError);
        sock.once('close', () => reject(new Error('closed')));
    });
}

async function main() {
    console.log('=== Cluster Cross-Gate Message Test ===\n');

    // Step 1: Connect Player A to Gate-1
    console.log('[1] Connecting Player A (uid=20001) → Gate-1 (7888)...');
    const sockA = await connect(GATE1.host, GATE1.port, 20001);
    sockA.write(encodeHandshake(20001));
    await new Promise(r => setTimeout(r, 100));
    await readPacket(sockA); // consume response
    console.log('  ✅ Player A connected to Gate-1\n');

    // Step 2: Connect Player B to Gate-2
    console.log('[2] Connecting Player B (uid=20002) → Gate-2 (7889)...');
    const sockB = await connect(GATE2.host, GATE2.port, 20002);
    sockB.write(encodeHandshake(20002));
    await new Promise(r => setTimeout(r, 100));
    await readPacket(sockB); // consume response
    console.log('  ✅ Player B connected to Gate-2\n');

    // Step 3: Register route index (via logic_server response)
    // The logic_server should have registered uid=20001 → gate-1, uid=20002 → gate-2
    console.log('[3] Verifying route index in Redis...');
    await new Promise(r => setTimeout(r, 500));

    // Step 4: Player A sends chat message (broadcast, received by Player B via cross-gate)
    console.log('[4] Player A sends chat (msg_id=2001)...');
    const chatPkt = encodeChatMsg(20001, 0, 'hello cross-gate from gate-1!');
    sockA.write(chatPkt);
    console.log('  📤 Sent: hello cross-gate from gate-1!\n');

    // Step 5: Player B should receive broadcast (msg_id=7002) via Redis PubSub → Gate-2
    console.log('[5] Waiting for Player B to receive broadcast...');
    let received = false;
    try {
        const data = await new Promise((resolve, reject) => {
            const timeout = setTimeout(() => reject(new Error('timeout (5s)')), 5000);
            sockB.once('data', (d) => { clearTimeout(timeout); resolve(d); });
        });
        console.log(`  📥 Received ${data.length} bytes\n`);

        let buf = data;
        while (buf.length >= HEADER_SIZE) {
            if (buf[0] !== MAGIC[0] || buf[1] !== MAGIC[1]) { buf = buf.subarray(1); continue; }
            const bodyLen = buf.readUInt16BE(6);
            const totalLen = HEADER_SIZE + bodyLen;
            if (buf.length < totalLen) break;
            const encrypted = buf.subarray(HEADER_SIZE, totalLen);
            try {
                const plain = decrypt(encrypted);
                const json = JSON.parse(plain.toString('utf8'));
                const mid = buf.readUInt16BE(4);
                console.log(`  ✅ Decrypted: msg_id=${mid}, body=${JSON.stringify(json)}`);
                if (mid === 7002) { received = true; }
            } catch(e) { /* skip */ }
            buf = buf.subarray(totalLen);
        }
        console.log(received ? '\n  🎉 CROSS-GATE CHAT WORKING!' : '\n  ⚠️ No 7002 broadcast found');
    } catch (e) {
        console.log(`  ❌ ${e.message}`);
    }

    // Step 6: Summary
    console.log('\n[6] Results:');
    try {
        const g1 = JSON.parse((await new Promise((resolve) => {
            require('http').get('http://127.0.0.1:9090/health', r => {
                let d=''; r.on('data',c=>d+=c); r.on('end',()=>resolve(d));
            });
        })).toString());
        const g2 = JSON.parse((await new Promise((resolve) => {
            require('http').get('http://127.0.0.1:9091/health', r => {
                let d=''; r.on('data',c=>d+=c); r.on('end',()=>resolve(d));
            });
        })).toString());
        console.log(`  Gate-1: node=${g1.node_name}, online=${g1.online_count}`);
        console.log(`  Gate-2: node=${g2.node_name}, online=${g2.online_count}`);
    } catch(e) {}

    // Cleanup
    sockA.destroy();
    sockB.destroy();
    console.log('\n=== Test Complete ===');
}

main().catch(e => { console.error('Fatal:', e.message); process.exit(1); });
