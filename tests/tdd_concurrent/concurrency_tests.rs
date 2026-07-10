//! TDD 并发测试 - 无锁容器并发安全
//!
//! 测试 DashMap、parking_lot 并发读写、无数据竞争

use std::sync::Arc;
use std::thread;

use rust_mmo_gate::security::ip_blacklist::IpBlacklist;
use rust_mmo_gate::security::rate_limit::RateLimiter;
use rust_mmo_gate::foundation::SnowflakeIdGen;

#[test]
fn test_concurrent_ip_blacklist() {
    let bl = Arc::new(IpBlacklist::new());
    let mut handles = vec![];

    for i in 0..8 {
        let b = bl.clone();
        handles.push(thread::spawn(move || {
            for j in 0..100 {
                let ip: std::net::IpAddr = format!("10.{}.{}.{}", i, j / 256, j % 256)
                    .parse()
                    .unwrap();
                b.block(ip);
                assert!(b.is_blocked(&ip));
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(bl.len(), 800);
}

#[test]
fn test_concurrent_rate_limiter() {
    let limiter = Arc::new(RateLimiter::new(1000, 2000, 100000));
    let mut handles = vec![];

    for _ in 0..8 {
        let l = limiter.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                l.check_player_rate(1, false);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
    // 不应 panic，无数据竞争
}

#[test]
fn test_concurrent_snowflake() {
    let gen = Arc::new(parking_lot::Mutex::new(SnowflakeIdGen::new(1).unwrap()));
    let mut handles = vec![];
    let mut all_ids = std::collections::HashSet::new();

    for _ in 0..8 {
        let g = gen.clone();
        let mut local_ids = vec![];
        handles.push(thread::spawn(move || {
            for _ in 0..1000 {
                local_ids.push(g.lock().next_id().unwrap());
            }
            local_ids
        }));
    }

    for h in handles {
        for id in h.join().unwrap() {
            assert!(all_ids.insert(id), "重复ID: {}", id);
        }
    }

    assert_eq!(all_ids.len(), 8000);
}
