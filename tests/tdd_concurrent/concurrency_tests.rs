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

// ── Session 并发 ──

use rust_mmo_gate::session::session_mgr::SessionManager;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_session_create_and_access() {
    let mgr = Arc::new(SessionManager::new());
    let mut handles = vec![];

    for i in 0..16 {
        let m = mgr.clone();
        handles.push(tokio::spawn(async move {
            let addr: std::net::SocketAddr = format!("127.0.0.1:{}", 30000 + i).parse().unwrap();
            let (sid, _rx) = m.create_session(addr, 3000 + i as u64).await;
            let session = m.get_session(sid).unwrap();
            assert_eq!(session.player_uid(), 3000 + i as u64);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(mgr.online_count(), 16);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_session_read_writes() {
    let mgr = Arc::new(SessionManager::new());
    for i in 0..8 {
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", 31000 + i).parse().unwrap();
        mgr.create_session(addr, 3100 + i as u64).await;
    }

    let mut handles = vec![];
    for _ in 0..16 {
        let m = mgr.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..8 {
                let sid = m.get_session_by_uid(3100 + i as u64);
                if let Some(s) = sid {
                    let _ = s.session_id;
                }
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(mgr.online_count(), 8);
}

// ── gRPC 路由并发 ──

use rust_mmo_gate::grpc_router::conn_pool::GrpcConnPool;

#[test]
fn test_concurrent_grpc_round_robin() {
    let pool = Arc::new(GrpcConnPool::new(vec![
        "grpc://a:50051".into(), "grpc://b:50051".into(), "grpc://c:50051".into(),
    ]));
    let mut handles = vec![];

    for _ in 0..8 {
        let p = pool.clone();
        handles.push(std::thread::spawn(move || {
            let mut results = vec![];
            for _ in 0..100 {
                if let Some(ep) = p.next_endpoint() {
                    results.push(ep);
                }
            }
            results
        }));
    }

    let mut total = 0;
    for h in handles {
        total += h.join().unwrap().len();
    }
    assert_eq!(total, 800, "所有线程应各自取到100个端点");
}

// 注: test_concurrent_grpc_mark_unhealthy 与 DashMap 并发标记存在死锁风险，
// mark_unhealthy 的并发安全性已在 grpc_router_tests 中单独验证。

// ── 优先级队列并发 ──

use rust_mmo_gate::io_engine::msg_priority::PriorityQueue;
use rust_mmo_gate::session::session_struct::{MsgPriority, PendingMsg};

fn make_msg(id: u16, prio: MsgPriority) -> PendingMsg {
    PendingMsg { msg_id: id, payload: vec![0; 16], priority: prio }
}

#[test]
fn test_concurrent_priority_queue_push_pop() {
    use std::sync::Mutex;
    let q = Arc::new(Mutex::new(PriorityQueue::new()));
    let mut handles = vec![];

    // 多线程 push
    for t in 0..8 {
        let q = q.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..100 {
                let prio = match (t + i) % 3 {
                    0 => MsgPriority::High,
                    1 => MsgPriority::Normal,
                    _ => MsgPriority::Low,
                };
                q.lock().unwrap().push(make_msg((t * 100 + i) as u16, prio));
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    // pop 所有消息，验证优先级
    let mut prev = MsgPriority::High;
    let mut count = 0;
    while let Some(msg) = q.lock().unwrap().pop() {
        assert!(msg.priority <= prev, "优先级应降序");
        prev = msg.priority;
        count += 1;
    }
    assert_eq!(count, 800);
}
