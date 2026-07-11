#!/usr/bin/env node
// WebSocket → TCP 代理
// 浏览器通过 ws://127.0.0.1:9000 连接，转发到 Rust Gateway TCP 7888
// 用法: node ws_proxy.js [--port 9000] [--gateway 127.0.0.1:7888]

const WebSocket = require('ws');
const net = require('net');

function getArg(name, def) {
    const i = process.argv.indexOf(name);
    return i !== -1 && i + 1 < process.argv.length ? process.argv[i + 1] : def;
}
const WS_PORT = parseInt(getArg('--port', '9000'));
const GATE = getArg('--gateway', '127.0.0.1:7888').split(':');
const GATE_HOST = GATE[0];
const GATE_PORT = parseInt(GATE[1] || '7888');

let connId = 0;

const wss = new WebSocket.Server({ port: WS_PORT });
console.log(`[WS Proxy] Listening on ws://127.0.0.1:${WS_PORT} → TCP ${GATE_HOST}:${GATE_PORT}`);

wss.on('connection', (ws, req) => {
    const id = ++connId;
    const clientIP = req.socket.remoteAddress;
    console.log(`[${id}] Browser connected from ${clientIP}`);

    const tcp = net.createConnection({ host: GATE_HOST, port: GATE_PORT }, () => {
        console.log(`[${id}] TCP connected to Gateway`);
    });

    // Browser → Gateway (binary)
    ws.on('message', (data) => {
        const buf = Buffer.from(data);
        if (!tcp.destroyed) {
            tcp.write(buf);
        }
    });

    // Gateway → Browser (binary)
    let tcpBytes = 0;
    tcp.on('data', (data) => {
        tcpBytes += data.length;
        if (ws.readyState === WebSocket.OPEN) {
            ws.send(data);
        }
    });

    // Cleanup
    ws.on('close', () => {
        console.log(`[${id}] Browser disconnected (TCP: ${tcpBytes} bytes)`);
        if (!tcp.destroyed) tcp.destroy();
    });
    tcp.on('close', () => {
        if (ws.readyState === WebSocket.OPEN) ws.close();
    });
    tcp.on('error', (e) => {
        console.log(`[${id}] TCP error: ${e.message}`);
        if (ws.readyState === WebSocket.OPEN) ws.close();
    });
    ws.on('error', (e) => {
        console.log(`[${id}] WS error: ${e.message}`);
        if (!tcp.destroyed) tcp.destroy();
    });
});

console.log('[WS Proxy] Ready. Open game.html in browser to connect.\n');
