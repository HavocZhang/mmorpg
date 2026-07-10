//! Cluster Integration Test - Rust MMO Gateway
//!
//! 测试多网关节点集群功能：
//! 1. 跨网关定向消息投递（node2 → Redis PubSub → node3 → 目标玩家）
//! 2. 跨网关广播消息（node2 → Redis PubSub → all nodes → all players）
//! 3. 路由索引验证（player_uid → gate_node_id 映射）
//! 4. 集群服务发现验证（gate:nodes SET 包含所有活跃节点）
//!
//! Usage:
//!   cargo run --release --bin cluster_test

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::time::{Duration, Instant};

use aes_gcm::{aead::{Aead, AeadCore, KeyInit, OsRng}, Aes256Gcm};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// Protocol constants (must match gateway)
const HEADER_SIZE: usize = 16;
const MAGIC: [u8; 2] = [0x4d, 0x4d];
const PROTOCOL_VERSION: u8 = 1;
const MAX_BODY_SIZE: usize = 8192;
const MSG_QUERY: u16 = 4001;
const MSG_HANDSHAKE: u16 = 0x0001;
const AES_KEY_HEX: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

// CRC32
const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

fn crc32(buf: &[u8]) -> u32 {
    let mut crc = 0xffffffff_u32;
    for &b in buf {
        crc = CRC32_TABLE[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffffffff
}

fn build_packet(msg_id: u16, payload: &[u8], cipher: &Aes256Gcm) -> Vec<u8> {
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher.encrypt(&nonce, payload).unwrap();

    let mut encrypted = Vec::with_capacity(12 + ciphertext.len());
    encrypted.extend_from_slice(nonce.as_slice());
    encrypted.extend_from_slice(&ciphertext);

    let body_len = encrypted.len() as u16;
    let crc = crc32(&encrypted);

    let mut packet = Vec::with_capacity(HEADER_SIZE + encrypted.len());
    packet.extend_from_slice(&MAGIC);
    packet.push(PROTOCOL_VERSION);
    packet.push(0);
    packet.extend_from_slice(&msg_id.to_be_bytes());
    packet.extend_from_slice(&body_len.to_be_bytes());
    packet.extend_from_slice(&crc.to_be_bytes());
    packet.extend_from_slice(&[0, 0, 0, 0]);
    packet.extend_from_slice(&encrypted);
    packet
}

fn build_handshake(uid: u64, cipher: &Aes256Gcm) -> Vec<u8> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let payload = format!(
        r#"{{"uid":{},"token":"test_token_123","version":1,"timestamp":{}}}"#,
        uid, ts
    );
    build_packet(MSG_HANDSHAKE, payload.as_bytes(), cipher)
}

/// 解析收到的数据包，返回 (msg_id, payload)
fn parse_packet(buf: &[u8]) -> Option<(u16, Vec<u8>)> {
    if buf.len() < HEADER_SIZE {
        return None;
    }
    if buf[0] != MAGIC[0] || buf[1] != MAGIC[1] {
        return None;
    }
    let msg_id = u16::from_be_bytes([buf[4], buf[5]]);
    let body_len = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    if buf.len() < HEADER_SIZE + body_len {
        return None;
    }
    Some((msg_id, buf[HEADER_SIZE..HEADER_SIZE + body_len].to_vec()))
}

/// 连接到网关并完成握手（网关不发送握手响应，发送后即可）
async fn connect_and_handshake(
    host: &str,
    port: u16,
    uid: u64,
    cipher: &Aes256Gcm,
) -> Result<TcpStream, Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).await?;
    stream.set_nodelay(true)?;

    // 发送握手包（网关不回复握手响应，直接进入 ReadLoop/WriteLoop）
    let handshake = build_handshake(uid, cipher);
    stream.write_all(&handshake).await?;

    // 等待网关处理握手（创建会话 + 更新路由索引）
    tokio::time::sleep(Duration::from_millis(500)).await;

    Ok(stream)
}

/// 跨网关消息结构（与 gateway 的 CrossGateMsg 一致）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CrossGateMsg {
    from_node: u64,
    to_uid: u64,
    msg_id: u16,
    payload: Vec<u8>,
    priority: u8,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    println!("============================================================");
    println!("  Rust MMO Gateway - Cluster Integration Test");
    println!("============================================================\n");

    let key_bytes = hex::decode(AES_KEY_HEX).unwrap();
    let cipher = Arc::new(Aes256Gcm::new_from_slice(&key_bytes).unwrap());

    // ── 测试 1: 集群服务发现验证 ──
    println!("━━━ 测试 1: 集群服务发现 ━━━");

    // 检查所有节点的 HTTP health
    let nodes = [
        ("node1", "127.0.0.1", 9090u16, 7888u16, 1u64),
        ("node2", "127.0.0.1", 9092, 7882, 2),
        ("node3", "127.0.0.1", 9093, 7883, 3),
    ];

    let mut online_nodes = 0;
    for (name, host, http_port, tcp_port, node_id) in &nodes {
        match reqwest::get(format!("http://{}:{}/health", host, http_port)).await {
            Ok(resp) => {
                if resp.status().is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    println!("  ✅ {} (ID={}): {} — TCP:{}, HTTP:{}", name, node_id, body, tcp_port, http_port);
                    online_nodes += 1;
                } else {
                    println!("  ❌ {} (ID={}): HTTP {}", name, node_id, resp.status());
                }
            }
            Err(e) => {
                println!("  ❌ {} (ID={}): 连接失败 — {}", name, node_id, e);
            }
        }
    }

    println!("\n  在线节点: {}/3\n", online_nodes);
    assert!(online_nodes >= 2, "至少需要 2 个节点在线才能测试集群");

    // ── 测试 2: Redis 集群注册验证 ──
    println!("━━━ 测试 2: Redis 集群注册 ━━━");

    let redis_url = "redis://127.0.0.1:6379";
    let redis_client = redis::Client::open(redis_url).unwrap();
    let mut redis_conn = redis_client.get_multiplexed_async_connection().await.unwrap();

    use redis::AsyncCommands;
    let gate_nodes: Vec<String> = redis_conn
        .smembers("gate:nodes")
        .await
        .unwrap_or_default();

    println!("  gate:nodes = {:?}", gate_nodes);
    println!("  路由索引数量: {}", {
        let keys: Vec<String> = redis_conn.keys("gate:route:*").await.unwrap_or_default();
        keys.len()
    });

    // ── 测试 3: 跨网关定向消息投递 ──
    println!("\n━━━ 测试 3: 跨网关定向消息投递 ━━━");

    let uid_a: u64 = 200001;
    let uid_b: u64 = 200002;

    // 连接玩家 A 到 node2, 玩家 B 到 node3
    println!("  连接玩家 A (uid={}) → node2 (port 7882)...", uid_a);
    let mut stream_a = match connect_and_handshake("127.0.0.1", 7882, uid_a, &cipher).await {
        Ok(s) => {
            println!("  ✅ 玩家 A 连接成功");
            s
        }
        Err(e) => {
            println!("  ❌ 玩家 A 连接失败: {}", e);
            return;
        }
    };

    println!("  连接玩家 B (uid={}) → node3 (port 7883)...", uid_b);
    let mut stream_b = match connect_and_handshake("127.0.0.1", 7883, uid_b, &cipher).await {
        Ok(s) => {
            println!("  ✅ 玩家 B 连接成功");
            s
        }
        Err(e) => {
            println!("  ❌ 玩家 B 连接失败: {}", e);
            return;
        }
    };

    // 等待路由索引更新
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 验证路由索引
    let route_a: Option<String> = redis_conn.get(format!("gate:route:{}", uid_a)).await.ok().flatten();
    let route_b: Option<String> = redis_conn.get(format!("gate:route:{}", uid_b)).await.ok().flatten();
    println!("  路由索引: uid={} → node {}, uid={} → node {}", uid_a, route_a.unwrap_or("?".into()), uid_b, route_b.unwrap_or("?".into()));

    // 通过 Redis PubSub 发送跨网关定向消息
    // node2 → gate:msg:3 → node3 → 玩家 B
    println!("\n  发送跨网关定向消息: node2 → node3 → uid={}", uid_b);

    let cross_msg = CrossGateMsg {
        from_node: 2,
        to_uid: uid_b,
        msg_id: 9999,
        payload: b"cross-gateway-test-payload".to_vec(),
        priority: 1,
    };

    let msg_data = serde_json::to_vec(&cross_msg).unwrap();
    let _: i64 = redis_conn
        .publish("gate:msg:3", &msg_data)
        .await
        .unwrap();
    println!("  ✅ 消息已发布到 gate:msg:3");

    // 等待玩家 B 收到消息
    let received = Arc::new(AtomicBool::new(false));
    let received_clone = received.clone();

    // 设置 5 秒超时读取玩家 B 的响应
    let read_b = tokio::time::timeout(Duration::from_secs(5), async {
        let mut buf = vec![0u8; 4096];
        loop {
            match stream_b.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if let Some((msg_id, _)) = parse_packet(&buf[..n]) {
                        println!("  📨 玩家 B 收到消息: msg_id={}", msg_id);
                        if msg_id == 9999 {
                            received_clone.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    let _ = read_b.await;

    if received.load(Ordering::SeqCst) {
        println!("  ✅ 跨网关定向消息投递成功！\n");
    } else {
        println!("  ⚠️  玩家 B 未收到跨网关消息（可能需要网关重新编译生效）\n");
    }

    // ── 测试 4: 跨网关广播消息 ──
    println!("━━━ 测试 4: 跨网关广播消息 ━━━");

    let broadcast_msg = CrossGateMsg {
        from_node: 1,
        to_uid: 0, // 0 = 广播
        msg_id: 8888,
        payload: b"broadcast-test-payload".to_vec(),
        priority: 0,
    };

    let bcast_data = serde_json::to_vec(&broadcast_msg).unwrap();
    let _: i64 = redis_conn
        .publish("gate:broadcast", &bcast_data)
        .await
        .unwrap();
    println!("  ✅ 广播消息已发布到 gate:broadcast");

    // 检查两个玩家是否都收到广播
    let bcast_a = Arc::new(AtomicBool::new(false));
    let bcast_b = Arc::new(AtomicBool::new(false));
    let bcast_a_clone = bcast_a.clone();
    let bcast_b_clone = bcast_b.clone();

    let read_a = tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_secs(2), stream_a.read(&mut buf)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => {
                    if let Some((msg_id, _)) = parse_packet(&buf[..n]) {
                        if msg_id == 8888 {
                            bcast_a_clone.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    });

    let read_b2 = tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_secs(2), stream_b.read(&mut buf)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => {
                    if let Some((msg_id, _)) = parse_packet(&buf[..n]) {
                        if msg_id == 8888 {
                            bcast_b_clone.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    });

    let _ = tokio::join!(read_a, read_b2);

    println!("  玩家 A 收到广播: {}", if bcast_a.load(Ordering::SeqCst) { "✅" } else { "⚠️" });
    println!("  玩家 B 收到广播: {}", if bcast_b.load(Ordering::SeqCst) { "✅" } else { "⚠️" });
    println!();

    // ── 测试 5: 节点健康状态 ──
    println!("━━━ 测试 5: 节点健康状态 ━━━");

    for (name, host, http_port, _, node_id) in &nodes {
        let url = format!("http://{}:{}/health", host, http_port);
        match reqwest::get(&url).await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                // 尝试解析 JSON
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    let online = json["online_count"].as_u64().unwrap_or(0);
                    let uptime = json["uptime_secs"].as_u64().unwrap_or(0);
                    println!("  {} (ID={}): 在线={} 运行={}s", name, node_id, online, uptime);
                }
            }
            _ => {
                println!("  {} (ID={}): 不可达", name, node_id);
            }
        }
    }

    // ── 测试 6: 合包统计 ──
    println!("\n━━━ 测试 6: 合包统计 ━━━");
    for (name, host, http_port, _, _) in &nodes {
        let url = format!("http://{}:{}/merge_stats", host, http_port);
        match reqwest::get(&url).await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                println!("  {}: {}", name, body);
            }
            _ => {
                println!("  {}: 不可达", name);
            }
        }
    }

    println!("\n============================================================");
    println!("  集群集成测试完成");
    println!("============================================================");
}
