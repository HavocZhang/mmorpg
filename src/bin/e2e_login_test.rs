//! 登录服端到端测试 — 通过网关 TCP 协议
//!
//! 测试完整链路: Client → Gateway(:7888) → gRPC → LoginServer(:50052)
//!
//! ```bash
//! # 1. 启动登录服: cargo run --bin login-server
//! # 2. 启动网关 (指向登录服): GRPC_LOGIC_ENDPOINTS=grpc://127.0.0.1:50052 cargo run --bin rust-mmo-gate
//! # 3. 运行测试: cargo run --bin e2e-login-test
//! ```

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use serde_json::Value;

const GATEWAY_ADDR: &str = "127.0.0.1:7888";
const AES_KEY: &[u8; 32] = b"\x00\x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa\xbb\xcc\xdd\xee\xff\x00\x11\x22\x33\x44\x55\x66\x77\x88\x99\xaa\xbb\xcc\xdd\xee\xff";
const PROTO_MAGIC: u16 = 0x4D4D;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 构造并加密一个协议包 (客户端格式)
/// 包体 = [12字节 nonce] [AES-256-GCM 密文 + 16字节 tag]
fn build_packet(msg_id: u16, body: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(AES_KEY).unwrap();
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = cipher.encrypt(nonce, body).expect("encrypt failed");
    // encrypted = [ciphertext + 16-byte tag]

    // 客户端包体格式: nonce(12) + encrypted(tag included)
    let body_with_nonce: Vec<u8> = nonce.iter().chain(encrypted.iter()).copied().collect();

    let mut packet = Vec::with_capacity(16 + body_with_nonce.len());
    packet.extend_from_slice(&PROTO_MAGIC.to_be_bytes());     // [0-1] magic
    packet.push(0x01);                                         // [2] version
    packet.push(0x00);                                         // [3] reserved
    packet.extend_from_slice(&msg_id.to_be_bytes());           // [4-5] msg_id
    packet.extend_from_slice(&(body_with_nonce.len() as u16).to_be_bytes()); // [6-7] body_len
    let crc = crc32fast::hash(&body_with_nonce);
    packet.extend_from_slice(&crc.to_be_bytes());              // [8-11] crc32
    packet.extend_from_slice(&0u32.to_be_bytes());             // [12-15] flags
    packet.extend_from_slice(&body_with_nonce);                // body

    packet
}

/// 解密响应包
/// 服务端加密格式: [12字节 nonce][AES-GCM密文+16字节tag]
fn decrypt_packet(data: &[u8]) -> Option<(u16, Vec<u8>)> {
    if data.len() < 16 {
        return None;
    }
    let msg_id = u16::from_be_bytes([data[4], data[5]]);
    let body_len = u16::from_be_bytes([data[6], data[7]]) as usize;
    let body_data = &data[16..16 + body_len.min(data.len() - 16)];

    // 格式: [nonce 12字节][密文+tag]
    if body_data.len() < 28 { return None; }
    let nonce = Nonce::from_slice(&body_data[..12]);
    let encrypted = &body_data[12..];  // [密文 + 16字节 tag]

    let cipher = Aes256Gcm::new_from_slice(AES_KEY).unwrap();
    let decrypted = cipher.decrypt(nonce, encrypted).ok()?;

    Some((msg_id, decrypted))
}

/// 发送请求并读取响应
fn request(msg_id: u16, body_json: &str) -> Result<(u16, Value), String> {
    let mut stream = TcpStream::connect(GATEWAY_ADDR)
        .map_err(|e| format!("TCP连接失败: {}", e))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {}", e))?;

    // 先发握手包 (msg_id=0, 握手)
    let handshake = serde_json::json!({
        "uid": 10001u64,
        "token": "test1234567890",
        "version": 1u32,
        "timestamp": now_secs(),
    });
    let hs_body = serde_json::to_vec(&handshake).unwrap();
    let hs_packet = build_packet(0, &hs_body);
    stream.write_all(&hs_packet).map_err(|e| format!("send handshake: {}", e))?;

    // 读取握手响应（网关可能不返回握手响应）
    // 短暂等待
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 发送业务消息
    let body = body_json.as_bytes();
    let packet = build_packet(msg_id, body);
    stream.write_all(&packet).map_err(|e| format!("send msg: {}", e))?;

    // 读取响应
    let mut header = [0u8; 16];
    stream.read_exact(&mut header).map_err(|e| format!("read header: {}", e))?;

    let body_len = u16::from_be_bytes([header[6], header[7]]) as usize;
    let mut enc_body = vec![0u8; body_len];
    stream.read_exact(&mut enc_body).map_err(|e| format!("read body: {}", e))?;

    // 构建完整包用于解密
    let full_packet: Vec<u8> = header.iter().chain(enc_body.iter()).copied().collect();
    let (resp_msg_id, decrypted) = decrypt_packet(&full_packet)
        .ok_or("解密失败")?;

    let json: Value = serde_json::from_slice(&decrypted)
        .map_err(|e| format!("JSON解析失败: {} body={:?}", e, String::from_utf8_lossy(&decrypted)))?;

    Ok((resp_msg_id, json))
}

fn main() {
    println!("╔══════════════════════════════════════════════╗");
    println!("║   登录服 端到端测试 (通过网关)               ║");
    println!("║   链路: Client → Gateway → gRPC → Login     ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();

    let mut passed = 0u32;
    let mut failed = 0u32;

    // ── 测试 1: 登录 ──
    println!("[测试] 登录 test/123456...");
    match request(101, r#"{"username":"test","password":"123456"}"#) {
        Ok((msg_id, json)) => {
            let success = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            let uid = json.get("uid").and_then(|v| v.as_u64()).unwrap_or(0);
            let token = json.get("token").and_then(|v| v.as_str()).unwrap_or("");
            if success && uid == 10001 && !token.is_empty() {
                println!("  ✅ PASS  登录成功: uid={} msg_id={}", uid, msg_id);
                passed += 1;
            } else {
                println!("  ❌ FAIL  登录: success={} uid={} token_len={}", success, uid, token.len());
                failed += 1;
            }
        }
        Err(e) => {
            println!("  ❌ FAIL  登录: {}", e);
            failed += 1;
        }
    }

    // ── 测试 2: 角色列表 ──
    println!("[测试] 角色列表...");
    match request(103, "{}") {
        Ok((msg_id, json)) => {
            let chars = json.get("chars").and_then(|v| v.as_array());
            match chars {
                Some(arr) => {
                    println!("  ✅ PASS  角色列表: {} 个角色 msg_id={}", arr.len(), msg_id);
                    passed += 1;
                }
                None => {
                    let success = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                    println!("  ❌ FAIL  角色列表: success={} json={}", success, json);
                    failed += 1;
                }
            }
        }
        Err(e) => {
            println!("  ❌ FAIL  角色列表: {}", e);
            failed += 1;
        }
    }

    // ── 测试 3: 创建角色 ──
    let char_name = format!("e2e_{}", now_secs() % 100000);
    println!("[测试] 创建角色 {}/warrior...", char_name);
    let create_json = serde_json::json!({"name": char_name, "class": "warrior"}).to_string();
    match request(104, &create_json) {
        Ok((msg_id, json)) => {
            let success = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            let char_id = json.get("char").and_then(|c| c.get("id")).and_then(|v| v.as_u64()).unwrap_or(0);
            if success && char_id > 0 {
                println!("  ✅ PASS  创建角色: charId={} msg_id={}", char_id, msg_id);
                passed += 1;
            } else {
                println!("  ❌ FAIL  创建角色: success={} charId={}", success, char_id);
                failed += 1;
            }
        }
        Err(e) => {
            println!("  ❌ FAIL  创建角色: {}", e);
            failed += 1;
        }
    }

    // ── 测试 4: 密码错误（应该失败） ──
    println!("[测试] 错误密码...");
    match request(101, r#"{"username":"test","password":"WRONG"}"#) {
        Ok((msg_id, json)) => {
            let err = json.get("error").and_then(|v| v.as_str()).unwrap_or("");
            if !err.is_empty() {
                println!("  ✅ PASS  密码错误: '{}' msg_id={}", err, msg_id);
                passed += 1;
            } else {
                println!("  ❌ FAIL  密码错误: 应该返回error但未返回");
                failed += 1;
            }
        }
        Err(e) => {
            println!("  ❌ FAIL  密码错误: {}", e);
            failed += 1;
        }
    }

    println!();
    println!("══════════════════════════════════════════════");
    println!("  结果: {} 通过, {} 失败, {} 总计", passed, failed, passed + failed);
    if failed == 0 {
        println!("  🎉 端到端测试全部通过!");
    }
    println!("══════════════════════════════════════════════");

    if failed > 0 {
        std::process::exit(1);
    }
}
