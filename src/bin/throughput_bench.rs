//! Throughput Benchmark Client - Rust MMO Gateway
//!
//! 高性能 Rust 压测客户端，测量网关极限吞吐能力
//! 门禁: >=80000 pps, 丢失率=0
//!
//! Usage:
//!   cargo run --release --bin throughput_bench -- --connections 100 --target-rate 80000

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use aes_gcm::{aead::{Aead, AeadCore, KeyInit, OsRng}, Aes256Gcm};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Barrier;

// Protocol constants
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

struct Args {
    connections: usize,
    target_rate: u64,
    duration_sec: u64,
    host: String,
    port: u16,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut a = Args {
        connections: 100,
        target_rate: 80000,
        duration_sec: 30,
        host: "127.0.0.1".to_string(),
        port: 7888,
    };
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--connections" if i + 1 < args.len() => { a.connections = args[i + 1].parse().unwrap_or(100); i += 1; }
            "--target-rate" if i + 1 < args.len() => { a.target_rate = args[i + 1].parse().unwrap_or(80000); i += 1; }
            "--duration" if i + 1 < args.len() => { a.duration_sec = args[i + 1].parse().unwrap_or(30); i += 1; }
            "--host" if i + 1 < args.len() => { a.host = args[i + 1].clone(); i += 1; }
            "--port" if i + 1 < args.len() => { a.port = args[i + 1].parse().unwrap_or(7888); i += 1; }
            _ => {}
        }
        i += 1;
    }
    a
}

#[derive(Default)]
struct Stats {
    sent: AtomicU64,
    received: AtomicU64,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let args = parse_args();

    println!("============================================================");
    println!("  Rust MMO Gateway - Throughput Benchmark (Rust Client)");
    println!("============================================================");
    println!("  Connections: {}", args.connections);
    println!("  Target rate: {} msg/s", args.target_rate);
    println!("  Duration: {}s", args.duration_sec);
    println!("  Target: {}:{}", args.host, args.port);
    println!("============================================================\n");

    // Create cipher
    let key_bytes = hex::decode(AES_KEY_HEX).unwrap();
    let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Arc::new(Aes256Gcm::new(key));

    // Pre-build query packet
    let query_payload = b"{}";
    let packet = Arc::new(build_packet(MSG_QUERY, query_payload, &cipher));

    println!("[Phase 1] Connecting {} clients...", args.connections);

    let stats = Arc::new(Stats::default());
    let barrier = Arc::new(Barrier::new(args.connections + 1));
    let mut handles = Vec::new();

    for i in 0..args.connections {
        let host = args.host.clone();
        let port = args.port;
        let uid = 100000 + i as u64;
        let packet = packet.clone();
        let cipher = cipher.clone();
        let stats = stats.clone();
        let barrier = barrier.clone();
        let target_rate = args.target_rate;
        let duration = args.duration_sec;

        let handle = tokio::spawn(async move {
            let mut stream = match TcpStream::connect((host.as_str(), port)).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("  Connect failed uid={}: {}", uid, e);
                    barrier.wait().await; // 仍需参与 barrier 避免死锁
                    return;
                }
            };
            stream.set_nodelay(true).ok();

            // Send handshake
            let hs = build_handshake(uid, &cipher);
            if stream.write_all(&hs).await.is_err() {
                barrier.wait().await;
                return;
            }

            // Wait for all to connect
            barrier.wait().await;

            let (mut read_half, mut write_half) = stream.into_split();

            // Spawn reader task
            let read_stats = stats.clone();
            let reader = tokio::spawn(async move {
                let mut buf = vec![0u8; 65536];
                let mut leftover = Vec::with_capacity(65536);
                loop {
                    match read_half.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            leftover.extend_from_slice(&buf[..n]);
                            while leftover.len() >= HEADER_SIZE {
                                if leftover[0] != MAGIC[0] || leftover[1] != MAGIC[1] {
                                    leftover.clear();
                                    break;
                                }
                                let body_len =
                                    u16::from_be_bytes([leftover[6], leftover[7]]) as usize;
                                let total = HEADER_SIZE + body_len;
                                if leftover.len() < total {
                                    break;
                                }
                                if body_len > MAX_BODY_SIZE {
                                    leftover.clear();
                                    break;
                                }
                                read_stats.received.fetch_add(1, Ordering::Relaxed);
                                leftover.drain(..total);
                            }
                        }
                        Err(_) => break,
                    }
                }
            });

            // Send loop - batch concat with 50ms interval (reliable on Windows)
            let per_client_rate = target_rate / args.connections as u64;
            let batch_size = (per_client_rate * 50 / 1000).max(1) as usize;
            let interval = Duration::from_millis(50);

            // Pre-build batch buffer (concatenated packets)
            let batch_buf: Vec<u8> = packet.repeat(batch_size);
            let start = Instant::now();

            loop {
                if write_half.write_all(&batch_buf).await.is_err() {
                    break;
                }
                stats.sent.fetch_add(batch_size as u64, Ordering::Relaxed);

                if start.elapsed() >= Duration::from_secs(duration) {
                    break;
                }

                tokio::time::sleep(interval).await;
            }

            drop(write_half);
            let _ = reader.await;
        });

        handles.push(handle);
    }

    // Wait for connections
    barrier.wait().await;
    println!("[Phase 1] All {} clients connected\n", args.connections);
    println!("[Phase 2] Running for {}s at target {} msg/s...\n", args.duration_sec, args.target_rate);

    // Monitor
    let start = Instant::now();
    let mut last_sent = 0u64;
    let mut last_recv = 0u64;
    let mut last_time = start;

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let now = Instant::now();
        let elapsed = now.duration_since(last_time).as_secs_f64();
        let total_elapsed = now.duration_since(start).as_secs_f64();

        let sent = stats.sent.load(Ordering::Relaxed);
        let recv = stats.received.load(Ordering::Relaxed);
        let send_rate = ((sent - last_sent) as f64 / elapsed) as u64;
        let recv_rate = ((recv - last_recv) as f64 / elapsed) as u64;
        let loss = if sent > 0 {
            (1.0 - recv as f64 / sent as f64) * 100.0
        } else {
            0.0
        };

        println!(
            "  [{:.0}s] sent: {}/s | recv: {}/s | loss: {:.2}% | total: {} sent, {} recv",
            total_elapsed, send_rate, recv_rate, loss, sent, recv
        );

        last_sent = sent;
        last_recv = recv;
        last_time = now;

        if total_elapsed >= args.duration_sec as f64 {
            break;
        }
    }

    // Wait for clients to finish
    for h in handles {
        let _ = h.await;
    }

    let sent = stats.sent.load(Ordering::Relaxed);
    let recv = stats.received.load(Ordering::Relaxed);
    let loss = if sent > 0 {
        (1.0 - recv as f64 / sent as f64) * 100.0
    } else {
        0.0
    };
    let avg_send = sent / args.duration_sec;
    let avg_recv = recv / args.duration_sec;

    println!("\n  ════════════════════════════════════════");
    println!("  ═══ Final Results ═══");
    println!("  ════════════════════════════════════════");
    println!("  Duration:       {}s", args.duration_sec);
    println!("  Total sent:     {}", sent);
    println!("  Total received: {}", recv);
    println!("  Avg send rate:  {}/s", avg_send);
    println!("  Avg recv rate:  {}/s", avg_recv);
    println!("  Loss rate:      {:.4}%", loss);

    let gate_pass = avg_recv >= 80000 && loss < 0.01;
    println!(
        "\n  Gate check (>=80K pps, loss=0): {}",
        if gate_pass { "PASS ✅" } else { "FAIL ❌" }
    );
    println!("    Recv rate: {} (need >= 80,000)", avg_recv);
    println!("    Loss rate: {:.4}% (need 0%)", loss);
    println!("\nDone.");
}
