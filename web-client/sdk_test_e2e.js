// E2E test: WS proxy → Gateway → Logic Server
const WebSocket = require('C:/Users/havoc/.workbuddy/binaries/node/workspace/node_modules/ws');
const crypto = require('crypto');

const AES_KEY = '00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff';
const V=1, HEAD=16, M=[0x4D,0x4D];
const cT=(()=>{const t=new Uint32Array(256);for(let i=0;i<256;i++){let c=i;for(let j=0;j<8;j++)c=c&1?0xedb88320^(c>>>1):c>>>1;t[i]=c>>>0;}return t;})();
function crc32(b){let c=0xffffffff;for(let i=0;i<b.length;i++)c=cT[(c^b[i])&0xff]^(c>>>8);return (c^0xffffffff)>>>0;}
function enc(p){
    const n=crypto.randomBytes(12);
    const ci=crypto.createCipheriv('aes-256-gcm',Buffer.from(AES_KEY,'hex'),n);
    const e=Buffer.concat([ci.update(p),ci.final()]);
    return Buffer.concat([n,e,ci.getAuthTag()]);
}
function pkt(m,p){
    const e=enc(typeof p==='string'?Buffer.from(p,'utf8'):p);
    const h=Buffer.alloc(HEAD);h[0]=M[0];h[1]=M[1];h[2]=V;h[3]=0;
    h.writeUInt16BE(m,4);h.writeUInt16BE(e.length,6);
    h.writeUInt32BE(crc32(e),8);h.writeUInt32BE(0,12);
    return Buffer.concat([h,e]);
}

const ws = new WebSocket('ws://127.0.0.1:9000');
let msgCount = 0;

ws.on('open', () => {
    console.log('WS connected OK');
    ws.send(pkt(1, JSON.stringify({uid:9999,token:'tok_12345678',version:V,timestamp:Math.floor(Date.now()/1000)})));
    console.log('Handshake sent');
    setTimeout(() => {
        ws.send(pkt(2001, JSON.stringify({from:9999,text:'hello world',channel:'world'})));
        console.log('Chat sent');
    }, 500);
    setTimeout(() => {
        ws.send(pkt(1001, JSON.stringify({skillId:1,targetUid:10000})));
        console.log('Attack sent');
    }, 1000);
    setTimeout(() => {
        console.log('Total msgs received:', msgCount);
        ws.close();
        process.exit(0);
    }, 3000);
});

ws.on('message', (data) => {
    msgCount++;
    const buf = Buffer.isBuffer(data) ? data : Buffer.from(data);
    if (buf[0]===M[0] && buf[1]===M[1]) {
        const msgId = buf.readUInt16BE(4);
        const names = {7002:'CHAT',6001:'BATTLE',8002:'ENTER',5001:'ATTR',5002:'EQUIP',5003:'INV',5004:'SKILLS',5005:'QUESTS',5006:'PLAYER'};
        console.log(`MSG #${msgCount}: ${names[msgId]||(''+msgId)} (${buf.length}B)`);
    }
});
ws.on('error', e => console.log('ERR:', e.message));
setTimeout(() => { console.log('TIMEOUT'); process.exit(1); }, 8000);
