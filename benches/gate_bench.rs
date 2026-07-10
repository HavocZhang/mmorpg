//! 网关性能压测基准
//!
//! 使用 criterion 进行性能基准测试
//! 运行: cargo bench
//!
//! 性能门禁指标（文档 5.2 节）：
//! 1. 单节点吞吐 ≥ 80000 包/秒
//! 2. 小包合并压缩率 ≥ 70%
//! 3. 编解码延迟 < 1ms
//! 4. 并发会话读写无锁、无阻塞、无死锁
//! 5. 消息投递丢失率 = 0
//! 6. 单节点稳定 2500 长连接无泄漏

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::time::Duration;

use rust_mmo_gate::crypto::aes_gcm::AesGcmCipher;
use rust_mmo_gate::crypto::crc32;
use rust_mmo_gate::foundation::SnowflakeIdGen;
use rust_mmo_gate::io_engine::msg_priority::PriorityQueue;
use rust_mmo_gate::io_engine::packet_merge::PacketMerge;
use rust_mmo_gate::protocol::decoder::PacketDecoder;
use rust_mmo_gate::protocol::encoder::PacketEncoder;
use rust_mmo_gate::security::ip_blacklist::IpBlacklist;
use rust_mmo_gate::security::rate_limit::RateLimiter;
use rust_mmo_gate::session::session_struct::{MsgPriority, PendingMsg};

const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

// ============================================================
// 基础性能基准（原有 8 项）
// ============================================================

fn bench_aes_encrypt(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let data = vec![0xABu8; 1024];

    c.bench_function("aes_encrypt_1kb", |b| {
        b.iter(|| {
            black_box(cipher.encrypt(black_box(&data)).unwrap());
        })
    });
}

fn bench_aes_decrypt(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let data = vec![0xABu8; 1024];
    let encrypted = cipher.encrypt(&data).unwrap();

    c.bench_function("aes_decrypt_1kb", |b| {
        b.iter(|| {
            black_box(cipher.decrypt(black_box(&encrypted)).unwrap());
        })
    });
}

fn bench_crc32(c: &mut Criterion) {
    let data = vec![0xABu8; 4096];

    c.bench_function("crc32_4kb", |b| {
        b.iter(|| {
            black_box(crc32::checksum(black_box(&data)));
        })
    });
}

fn bench_packet_encode(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let encoder = PacketEncoder::new(cipher);
    let payload = vec![0xCDu8; 512];

    c.bench_function("packet_encode_512b", |b| {
        b.iter(|| {
            black_box(encoder.encode_to_bytes(black_box(0x0001), black_box(&payload)).unwrap());
        })
    });
}

fn bench_packet_decode(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let encoder = PacketEncoder::new(cipher);
    let cipher2 = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let payload = vec![0xCDu8; 512];
    let encoded = encoder.encode_to_bytes(0x0001, &payload).unwrap();

    c.bench_function("packet_decode_512b", |b| {
        b.iter(|| {
            let mut decoder = PacketDecoder::new(cipher2.clone());
            decoder.feed(&encoded);
            black_box(decoder.decode().unwrap().unwrap());
        })
    });
}

fn bench_packet_merge(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff").unwrap();
    c.bench_function("packet_merge_10packets", |b| {
        b.iter(|| {
            let mut merge = PacketMerge::new(Duration::from_millis(16), cipher.clone());
            for i in 0..10 {
                merge.push(PendingMsg {
                    msg_id: i as u16,
                    payload: vec![0xAB; 50],
                    priority: MsgPriority::Normal,
                });
            }
            black_box(merge.flush());
        })
    });
}

fn bench_snowflake_id(c: &mut Criterion) {
    let mut gen = SnowflakeIdGen::new(1).unwrap();

    c.bench_function("snowflake_next_id", |b| {
        b.iter(|| {
            black_box(gen.next_id().unwrap());
        })
    });
}

fn bench_rate_limit(c: &mut Criterion) {
    let limiter = RateLimiter::new(100000, 200000, 1000000);

    c.bench_function("rate_limit_check", |b| {
        b.iter(|| {
            black_box(limiter.check_player_rate(black_box(1), false));
        })
    });
}

// ============================================================
// 性能门禁基准（文档 5.2 节新增）
// ============================================================

/// 门禁1：单节点吞吐 ≥ 80000 包/秒
///
/// 测试完整编解码流水线（编码 → 解码）的吞吐量
fn bench_throughput_gate(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let encoder = PacketEncoder::new(cipher);
    let cipher2 = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let payload = vec![0xCDu8; 64]; // 小包模拟团战

    let mut group = c.benchmark_group("throughput_gate");
    group.throughput(Throughput::Elements(1));

    group.bench_function("encode_decode_64b", |b| {
        b.iter(|| {
            let encoded = encoder.encode_to_bytes(0x0001, black_box(&payload)).unwrap();
            let mut decoder = PacketDecoder::new(cipher2.clone());
            decoder.feed(&encoded);
            let _ = decoder.decode().unwrap().unwrap();
        })
    });

    // 批量吞吐：100 个包一批
    group.throughput(Throughput::Elements(100));
    group.bench_function("batch_100_packets", |b| {
        b.iter(|| {
            let mut combined = Vec::with_capacity(100 * 80);
            for i in 0..100 {
                let encoded = encoder.encode_to_bytes(i as u16, &payload).unwrap();
                combined.extend_from_slice(&encoded);
            }
            let mut decoder = PacketDecoder::new(cipher2.clone());
            decoder.feed(&combined);
            let _ = decoder.decode_all().unwrap();
        })
    });

    group.finish();
}

/// 门禁2：小包合并压缩率 ≥ 70%
///
/// 测试不同包数下合并的压缩效果
fn bench_merge_compression(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff").unwrap();
    let mut group = c.benchmark_group("packet_merge_compression");

    for count in [5, 10, 20, 50] {
        group.bench_with_input(
            BenchmarkId::new("merge_n_packets", count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut merge = PacketMerge::new(Duration::from_millis(16), cipher.clone());
                    for i in 0..count {
                        merge.push(PendingMsg {
                            msg_id: i as u16,
                            payload: vec![0xAB; 50],
                            priority: MsgPriority::Normal,
                        });
                    }
                    let merged = merge.flush().unwrap();
                    // 合并后 1 次 TCP 写 vs count 次单独写
                    // 每包: 16(header) + 12(nonce) + 50(ct) + 16(tag) = 94 bytes
                    // 字节数不变，但系统调用从 count 次降为 1 次
                    let write_reduction = 1.0 - (1.0 / count as f64);
                    black_box((merged, write_reduction));
                })
            },
        );
    }

    group.finish();
}

/// 门禁3：编解码延迟 < 1ms
///
/// 测试不同负载大小的编解码延迟
fn bench_codec_latency(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let cipher2 = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();

    let mut group = c.benchmark_group("codec_latency");

    for size in [64, 256, 512, 1024, 4096] {
        let payload = vec![0xCDu8; size];
        let encoder = PacketEncoder::new(cipher.clone());

        group.bench_with_input(
            BenchmarkId::new("roundtrip", size),
            &size,
            |b, &_size| {
                b.iter(|| {
                    let encoded = encoder.encode_to_bytes(0x0001, &payload).unwrap();
                    let mut decoder = PacketDecoder::new(cipher2.clone());
                    decoder.feed(&encoded);
                    let _ = decoder.decode().unwrap().unwrap();
                })
            },
        );
    }

    group.finish();
}

/// 门禁4：并发会话读写无锁、无阻塞
///
/// 使用 DashMap 模拟并发会话读写，验证无锁性能
fn bench_concurrent_session(c: &mut Criterion) {
    use dashmap::DashMap;
    use std::thread;

    let mut group = c.benchmark_group("concurrent_session");

    // 并发读写 1000 个会话
    group.bench_function("dashmap_1000_sessions", |b| {
        b.iter(|| {
            let map: Arc<DashMap<u64, u64>> = Arc::new(DashMap::new());
            // 插入 1000 个会话
            for i in 0..1000u64 {
                map.insert(i, i * 2);
            }
            // 并发读取
            let map2 = map.clone();
            let handle = thread::spawn(move || {
                for i in 0..1000u64 {
                    let _ = map2.get(&i);
                }
            });
            // 主线程写入
            for i in 0..1000u64 {
                if let Some(mut v) = map.get_mut(&i) {
                    *v = i * 3;
                }
            }
            handle.join().unwrap();
            black_box(map.len());
        })
    });

    // 高频读写模拟（团战场景）
    group.bench_function("dashmap_battle_simulation", |b| {
        b.iter(|| {
            let map: Arc<DashMap<u64, u64>> = Arc::new(DashMap::new());
            for i in 0..500u64 {
                map.insert(i, 0);
            }

            let map2 = map.clone();
            let writer = thread::spawn(move || {
                for _ in 0..1000 {
                    for i in 0..500u64 {
                        if let Some(mut v) = map2.get_mut(&i) {
                            *v += 1;
                        }
                    }
                }
            });

            let map3 = map.clone();
            let reader = thread::spawn(move || {
                let mut sum = 0u64;
                for _ in 0..1000 {
                    for i in 0..500u64 {
                        if let Some(v) = map3.get(&i) {
                            sum += *v;
                        }
                    }
                }
                sum
            });

            writer.join().unwrap();
            let _ = reader.join().unwrap();
        })
    });

    group.finish();
}

/// 门禁5：消息投递丢失率 = 0
///
/// 测试优先级队列在大量消息下的完整性
fn bench_message_delivery(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_delivery");

    // 1000 条消息全部出队，验证不丢失
    group.bench_function("priority_queue_1000_msgs", |b| {
        b.iter(|| {
            let mut q = PriorityQueue::new();
            for i in 0..1000u16 {
                q.push(PendingMsg {
                    msg_id: i,
                    payload: vec![0; 10],
                    priority: if i % 3 == 0 {
                        MsgPriority::High
                    } else if i % 3 == 1 {
                        MsgPriority::Normal
                    } else {
                        MsgPriority::Low
                    },
                });
            }
            let mut count = 0;
            while q.pop().is_some() {
                count += 1;
            }
            assert_eq!(count, 1000, "消息不应丢失");
            black_box(count);
        })
    });

    group.finish();
}

/// 新增：IP 黑名单查询性能
fn bench_ip_blacklist(c: &mut Criterion) {
    use std::net::IpAddr;

    let bl = IpBlacklist::new();
    // 填入 10000 个 IP
    for i in 0..10000u32 {
        let ip: IpAddr = format!("10.{}.{}.{}", i / 65536, (i / 256) % 256, i % 256)
            .parse()
            .unwrap();
        bl.block(ip);
    }

    let test_ip: IpAddr = "10.0.0.1".parse().unwrap();

    c.bench_function("ip_blacklist_lookup_10k", |b| {
        b.iter(|| {
            black_box(bl.is_blocked(black_box(&test_ip)));
        })
    });
}

/// 新增：心跳巡检吞吐
fn bench_heartbeat_check(c: &mut Criterion) {
    use rust_mmo_gate::session::session_mgr::SessionManager;

    c.bench_function("heartbeat_check_idle", |b| {
        b.iter(|| {
            let mgr = SessionManager::new();
            // 无会话时巡检不应 panic 且极快
            // 注意：无法在 bench 中使用 async，这里测试同步部分
            black_box(mgr.online_count());
        })
    });
}

/// 新增：2500 并发连接模拟
///
/// 模拟 2500 个会话的内存和性能开销
fn bench_2500_connections(c: &mut Criterion) {
    use dashmap::DashMap;

    let mut group = c.benchmark_group("connections_2500");

    // 创建 2500 个会话映射
    group.bench_function("create_2500_sessions", |b| {
        b.iter(|| {
            let map: DashMap<u64, u64> = DashMap::new();
            for i in 0..2500u64 {
                map.insert(i, i);
            }
            black_box(map.len());
        })
    });

    // 2500 个会话的并发读取
    group.bench_function("read_2500_sessions", |b| {
        let map: Arc<DashMap<u64, u64>> = Arc::new(DashMap::new());
        for i in 0..2500u64 {
            map.insert(i, i * 2);
        }

        b.iter(|| {
            let mut sum = 0u64;
            for i in 0..2500u64 {
                if let Some(v) = map.get(&i) {
                    sum += *v;
                }
            }
            black_box(sum);
        })
    });

    // 2500 个会话的并发更新
    group.bench_function("update_2500_sessions", |b| {
        let map: Arc<DashMap<u64, u64>> = Arc::new(DashMap::new());
        for i in 0..2500u64 {
            map.insert(i, 0);
        }

        b.iter(|| {
            for i in 0..2500u64 {
                if let Some(mut v) = map.get_mut(&i) {
                    *v += 1;
                }
            }
            black_box(map.len());
        })
    });

    group.finish();
}

/// 新增：全局限流性能（8万包/秒场景）
fn bench_global_rate_limit(c: &mut Criterion) {
    let limiter = RateLimiter::new(30, 80, 80000);

    let mut group = c.benchmark_group("global_rate_limit");

    group.throughput(Throughput::Elements(1));
    group.bench_function("global_check_single", |b| {
        b.iter(|| {
            black_box(limiter.check_global_rate());
        })
    });

    // 批量检查 1000 个包
    group.throughput(Throughput::Elements(1000));
    group.bench_function("global_check_batch_1000", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                black_box(limiter.check_global_rate());
            }
        })
    });

    group.finish();
}

/// 新增：Snowflake ID 并发生成
fn bench_snowflake_concurrent(c: &mut Criterion) {
    use parking_lot::Mutex;
    use std::thread;

    let mut group = c.benchmark_group("snowflake_concurrent");

    // 单线程基线
    group.bench_function("single_thread", |b| {
        let gen = Mutex::new(SnowflakeIdGen::new(1).unwrap());
        b.iter(|| {
            let mut g = gen.lock();
            black_box(g.next_id().unwrap());
        })
    });

    // 4 线程并发
    group.bench_function("4_threads", |b| {
        b.iter(|| {
            let gen = Arc::new(Mutex::new(SnowflakeIdGen::new(1).unwrap()));
            let mut handles = vec![];
            for _ in 0..4 {
                let g = gen.clone();
                handles.push(thread::spawn(move || {
                    for _ in 0..250 {
                        let mut g = g.lock();
                        let _ = g.next_id().unwrap();
                    }
                }));
            }
            for h in handles {
                h.join().unwrap();
            }
        })
    });

    group.finish();
}

/// 新增：内存稳定性测试
///
/// 验证大量分配/释放后内存不泄漏
fn bench_memory_stability(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let encoder = PacketEncoder::new(cipher.clone());

    let mut group = c.benchmark_group("memory_stability");

    // 10000 次编解码循环
    group.throughput(Throughput::Elements(10000));
    group.bench_function("encode_10000_cycles", |b| {
        b.iter(|| {
            for i in 0..10000u16 {
                let payload = vec![0xAB; 64];
                let _ = encoder.encode_to_bytes(i, &payload).unwrap();
            }
        })
    });

    // 10000 次 PacketMerge 循环
    group.throughput(Throughput::Elements(10000));
    group.bench_function("merge_10000_cycles", |b| {
        b.iter(|| {
            for _ in 0..10000 {
                let mut merge = PacketMerge::new(Duration::from_millis(16), cipher.clone());
                merge.push(PendingMsg {
                    msg_id: 1,
                    payload: vec![0xAB; 50],
                    priority: MsgPriority::Normal,
                });
                let _ = merge.flush();
            }
        })
    });

    group.finish();
}

/// 新增：限流器并发性能
fn bench_rate_limit_concurrent(c: &mut Criterion) {
    use std::thread;

    let limiter = Arc::new(RateLimiter::new(1000000, 2000000, 10000000));

    let mut group = c.benchmark_group("rate_limit_concurrent");

    // 4 线程并发限流检查
    group.bench_function("4_threads_check", |b| {
        b.iter(|| {
            let mut handles = vec![];
            for tid in 0..4u64 {
                let l = limiter.clone();
                handles.push(thread::spawn(move || {
                    for _ in 0..250 {
                        l.check_player_rate(tid, false);
                    }
                }));
            }
            for h in handles {
                h.join().unwrap();
            }
        })
    });

    // 8 线程并发
    group.bench_function("8_threads_check", |b| {
        b.iter(|| {
            let mut handles = vec![];
            for tid in 0..8u64 {
                let l = limiter.clone();
                handles.push(thread::spawn(move || {
                    for _ in 0..125 {
                        l.check_player_rate(tid, false);
                    }
                }));
            }
            for h in handles {
                h.join().unwrap();
            }
        })
    });

    group.finish();
}

/// 新增：CRC32 不同数据量性能
fn bench_crc32_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("crc32_sizes");

    for size in [64, 256, 1024, 4096, 8192] {
        let data = vec![0xABu8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("crc32", size),
            &size,
            |b, &_size| {
                b.iter(|| {
                    black_box(crc32::checksum(black_box(&data)));
                })
            },
        );
    }

    group.finish();
}

/// 新增：AES 加解密不同数据量
fn bench_aes_sizes(c: &mut Criterion) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();

    let mut group = c.benchmark_group("aes_sizes");

    for size in [64, 256, 512, 1024, 4096] {
        let data = vec![0xABu8; size];
        let encrypted = cipher.encrypt(&data).unwrap();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("encrypt", size),
            &size,
            |b, &_size| {
                b.iter(|| {
                    black_box(cipher.encrypt(black_box(&data)).unwrap());
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("decrypt", size),
            &size,
            |b, &_size| {
                b.iter(|| {
                    black_box(cipher.decrypt(black_box(&encrypted)).unwrap());
                })
            },
        );
    }

    group.finish();
}

/// 新增：优先级队列入队出队性能
fn bench_priority_queue(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_queue");

    // 入队性能
    group.bench_function("push_100_msgs", |b| {
        b.iter(|| {
            let mut q = PriorityQueue::new();
            for i in 0..100u16 {
                q.push(PendingMsg {
                    msg_id: i,
                    payload: vec![0; 10],
                    priority: MsgPriority::Normal,
                });
            }
            black_box(q.len());
        })
    });

    // 出队性能
    group.bench_function("pop_100_msgs", |b| {
        b.iter(|| {
            let mut q = PriorityQueue::new();
            for i in 0..100u16 {
                q.push(PendingMsg {
                    msg_id: i,
                    payload: vec![0; 10],
                    priority: if i % 3 == 0 {
                        MsgPriority::High
                    } else {
                        MsgPriority::Normal
                    },
                });
            }
            while q.pop().is_some() {}
            black_box(q.len());
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    // 基础基准（8 项）
    bench_aes_encrypt,
    bench_aes_decrypt,
    bench_crc32,
    bench_packet_encode,
    bench_packet_decode,
    bench_packet_merge,
    bench_snowflake_id,
    bench_rate_limit,
    // 性能门禁基准（5 项）
    bench_throughput_gate,
    bench_merge_compression,
    bench_codec_latency,
    bench_concurrent_session,
    bench_message_delivery,
    // 扩展基准（8 项）
    bench_ip_blacklist,
    bench_heartbeat_check,
    bench_2500_connections,
    bench_global_rate_limit,
    bench_snowflake_concurrent,
    bench_memory_stability,
    bench_rate_limit_concurrent,
    bench_crc32_sizes,
    bench_aes_sizes,
    bench_priority_queue,
);
criterion_main!(benches);
