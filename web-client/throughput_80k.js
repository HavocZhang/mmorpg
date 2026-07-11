#!/usr/bin/env node
// 80K 吞吐压测 — 正确协议格式
const net = require('net'), crypto = require('crypto');

const HOST = process.argv.includes('--host') ? process.argv[process.argv.indexOf('--host')+1] : '127.0.0.1';
const PORT = parseInt(process.argv.includes('--port') ? process.argv[process.argv.indexOf('--port')+1] : '7888');
const CONNS = parseInt(process.argv.includes('--conns') ? process.argv[process.argv.indexOf('--conns')+1] : '1000');
const DUR = parseInt(process.argv.includes('--dur') ? process.argv[process.argv.indexOf('--dur')+1] : '15');

const AES_KEY = Buffer.from('00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff', 'hex');
const MAGIC = [0x4D, 0x4D], PROTOCOL_VERSION = 1, HEADER_SIZE = 16;

// CRC32 table
const crc32Table = (() => { const t = new Uint32Array(256); for (let i=0;i<256;i++){let c=i;for(let j=0;j<8;j++)c=c&1?0xedb88320^(c>>>1):c>>>1;t[i]=c>>>0;} return t; })();
function crc32(buf) { let c=0xffffffff; for(let i=0;i<buf.length;i++)c=crc32Table[(c^buf[i])&0xff]^(c>>>8); return (c^0xffffffff)>>>0; }

function encrypt(plaintext) {
    const nonce = crypto.randomBytes(12);
    const cipher = crypto.createCipheriv('aes-256-gcm', AES_KEY, nonce);
    const enc = Buffer.concat([cipher.update(plaintext), cipher.final()]);
    return Buffer.concat([nonce, enc, cipher.getAuthTag()]);
}

function encodePacket(msgId, plaintext) {
    const encrypted = encrypt(plaintext);
    const header = Buffer.alloc(HEADER_SIZE);
    header[0]=MAGIC[0]; header[1]=MAGIC[1];
    header[2]=PROTOCOL_VERSION; header[3]=0;
    header.writeUInt16BE(msgId, 4);
    header.writeUInt16BE(encrypted.length, 6);
    header.writeUInt32BE(crc32(encrypted), 8);
    header.writeUInt32BE(0, 12);
    return Buffer.concat([header, encrypted]);
}

function encodeHandshake(uid) {
    return encodePacket(0x0001, Buffer.from(JSON.stringify({
        uid, token: 'bench_token_00000000000000000000', version: PROTOCOL_VERSION,
        timestamp: Math.floor(Date.now()/1000)
    }), 'utf8'));
}

function encodeEcho(uid) {
    return encodePacket(0x1001, Buffer.from(JSON.stringify({uid, msg:'bench', t:Date.now()}), 'utf8'));
}

// ========== MAIN ==========
async function main() {
    console.log(`\nTHROUGHPUT BENCH: ${CONNS} conns × ${DUR}s → ${HOST}:${PORT}\n`);

    // Phase 1: Connect
    console.log('[Phase 1] Connecting...');
    let connected = 0, errors = 0;
    const socks = [];
    const t0 = Date.now();

    for (let i = 0; i < CONNS; i++) {
        const uid = 60000 + i;
        try {
            const sock = await new Promise((resolve, reject) => {
                const s = net.createConnection({host:HOST, port:PORT}, () => resolve(s));
                s.on('error', () => { s.destroy(); reject(); });
                s.setTimeout(10000, () => { s.destroy(); reject(new Error('timeout')); });
            });
            sock.setTimeout(0);
            sock.on('error', () => {}); // swallow errors silently
            const hs = encodeHandshake(uid);
            sock.write(hs);
            socks.push(sock);
            connected++;
        } catch(e) { errors++; }
        if (i % 500 === 499) console.log(`  ${i+1}/${CONNS} connected...`);
    }
    const connectTime = ((Date.now()-t0)/1000).toFixed(1);
    console.log(`  ${connected} connected in ${connectTime}s (${Math.round(connected/parseFloat(connectTime))} conn/s), ${errors} failed\n`);

    if (connected === 0) { console.log('All failed. Exiting.'); process.exit(1); }

    // Phase 2: Burst send
    console.log(`[Phase 2] Burst sending for ${DUR}s...\n`);
    let sent = 0, errs = 0, startTime = Date.now(), lastReport = startTime, lastSent = 0;
    const endTime = startTime + DUR * 1000;

    while (Date.now() < endTime) {
        for (let i = 0; i < socks.length; i++) {
            const s = socks[i];
            if (!s.destroyed) {
                try { s.write(encodeEcho(60000 + i)); sent++; }
                catch(e) { errs++; }
            }
            // Yield to event loop every 5000 packets
            if (sent % 5000 === 0) await new Promise(r => setImmediate(r));
        }

        const now = Date.now();
        if (now - lastReport >= 1000) {
            const elapsed = (now - lastReport) / 1000;
            const rate = Math.round((sent - lastSent) / elapsed);
            const totalTime = ((now - startTime) / 1000).toFixed(0);
            console.log(`  [${totalTime}s] ${rate.toLocaleString()} pps | total: ${sent.toLocaleString()} | errs: ${errs}`);
            lastSent = sent; lastReport = now;
            if (rate < 100 && totalTime > 5) { console.log('  Rate dropped too low, stopping.'); break; }
        }
    }

    const totalTime = (Date.now() - startTime) / 1000;
    const avgRate = Math.round(sent / totalTime);
    const peakRate = Math.round(sent / Math.min(totalTime, DUR));
    const passed = avgRate >= 80000;

    console.log(`\n========================================`);
    console.log(`  RESULTS`);
    console.log(`  Connections: ${connected}  |  Duration: ${totalTime.toFixed(1)}s`);
    console.log(`  Sent: ${sent.toLocaleString()}  |  Errors: ${errs}`);
    console.log(`  Avg Rate: ${avgRate.toLocaleString()} pps  |  Peak: ${peakRate.toLocaleString()} pps`);
    console.log(`  Gate: ${passed ? 'PASS (>=80K)' : avgRate>=40000 ? 'GOOD (>=40K)' : 'LOW'}`);
    console.log(`========================================\n`);

    for (const s of socks) try { s.destroy(); } catch(e) {}
}

main().catch(e => { console.error('Fatal:', e); process.exit(1); });
