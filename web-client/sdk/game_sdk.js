// MMO Game SDK — 浏览器版
// 连接 WS Proxy → Gateway → Logic Server
class GameSDK {
    constructor() {
        this.ws = null;
        this.uid = null;
        this.aesKey = null;
        this._handlers = {};
        this._connected = false;
    }

    // ── CRC32 ──
    static _crc32Table = (() => {
        const t = new Uint32Array(256);
        for (let i = 0; i < 256; i++) {
            let c = i;
            for (let j = 0; j < 8; j++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
            t[i] = c >>> 0;
        }
        return t;
    })();
    static crc32(buf) {
        let c = 0xffffffff;
        const t = GameSDK._crc32Table;
        for (let i = 0; i < buf.length; i++) c = t[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
        return (c ^ 0xffffffff) >>> 0;
    }

    // ── AES-256-GCM (browser crypto.subtle) ──
    static async _importKey(keyHex) {
        const raw = new Uint8Array(keyHex.match(/.{2}/g).map(b => parseInt(b, 16)));
        return crypto.subtle.importKey('raw', raw, { name: 'AES-GCM' }, false, ['encrypt', 'decrypt']);
    }
    async encrypt(plaintext) {
        const nonce = crypto.getRandomValues(new Uint8Array(12));
        const data = typeof plaintext === 'string' ? new TextEncoder().encode(plaintext) : plaintext;
        const enc = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nonce }, this._key, data);
        const tagOffset = enc.byteLength - 16;
        const ciphertext = new Uint8Array(enc, 0, tagOffset);
        const tag = new Uint8Array(enc, tagOffset, 16);
        const result = new Uint8Array(12 + ciphertext.length + 16);
        result.set(nonce, 0);
        result.set(ciphertext, 12);
        result.set(tag, 12 + ciphertext.length);
        return result;
    }
    async decrypt(data) {
        const nonce = data.subarray(0, 12);
        const tag = data.subarray(data.length - 16);
        const ciphertext = data.subarray(12, data.length - 16);
        const combined = new Uint8Array(ciphertext.length + tag.length);
        combined.set(ciphertext, 0);
        combined.set(tag, ciphertext.length);
        const dec = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: nonce }, this._key, combined);
        return new Uint8Array(dec);
    }

    // ── 协议编解码 ──
    async encodePacket(msgId, payload) {
        const body = typeof payload === 'string' ? new TextEncoder().encode(payload) : payload;
        const encrypted = await this.encrypt(body);
        const header = new Uint8Array(16);
        const view = new DataView(header.buffer);
        header[0] = 0x4D; header[1] = 0x4D;  // magic
        header[2] = 1; header[3] = 0;          // version, reserved
        view.setUint16(4, msgId, false);
        view.setUint16(6, encrypted.length, false);
        view.setUint32(8, GameSDK.crc32(encrypted), false);
        view.setUint32(12, 0, false);
        const result = new Uint8Array(16 + encrypted.length);
        result.set(header, 0);
        result.set(encrypted, 16);
        return result;
    }

    // ── WebSocket 连接 ──
    async connect(url, uid, aesKeyHex) {
        this.uid = uid;
        this.aesKey = aesKeyHex;
        this._key = await GameSDK._importKey(aesKeyHex);

        return new Promise((resolve, reject) => {
            this.ws = new WebSocket(url);
            this.ws.binaryType = 'arraybuffer';

            this.ws.onopen = async () => {
                // 握手: msg_id=1
                const handshake = await this.encodePacket(1, JSON.stringify({
                    uid, token: 'html_gametk_000',
                    version: 1, timestamp: Math.floor(Date.now() / 1000)
                }));
                this.ws.send(handshake);
                this._connected = true;
                resolve();
            };

            this.ws.onmessage = (event) => {
                this._onData(new Uint8Array(event.data));
            };

            this.ws.onerror = (e) => { console.error('ws error', e); reject(e); };
            this.ws.onclose = () => { this._connected = false; this.emit('close'); };
        });
    }

    // ── 发送消息 ──
    async send(msgId, payload) {
        if (!this._connected) return;
        const pkt = await this.encodePacket(msgId, payload);
        this.ws.send(pkt);
    }

    // ── 事件系统 ──
    on(event, fn) {
        if (!this._handlers[event]) this._handlers[event] = [];
        this._handlers[event].push(fn);
    }
    emit(event, data) {
        (this._handlers[event] || []).forEach(fn => fn(data));
    }

    // ── 接收消息解析 ──
    _onData(data) {
        let offset = 0;
        while (offset + 16 <= data.length) {
            if (data[offset] !== 0x4D || data[offset+1] !== 0x4D) { offset++; continue; }
            const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
            const bodyLen = view.getUint16(offset + 6, false);
            if (offset + 16 + bodyLen > data.length) break;
            const msgId = view.getUint16(offset + 4, false);
            const encrypted = data.subarray(offset + 16, offset + 16 + bodyLen);
            this.decrypt(encrypted).then(plain => {
                const text = new TextDecoder().decode(plain);
                try {
                    const json = JSON.parse(text);
                    this.emit('msg', { msgId, body: json });
                    this.emit(`msg:${msgId}`, json);
                } catch(e) {
                    this.emit('msg', { msgId, raw: text });
                }
            }).catch(() => {}); // 解密失败静默跳过
            offset += 16 + bodyLen;
        }
    }

    // ── 断开 ──
    close() { if (this.ws) this.ws.close(); }
}
