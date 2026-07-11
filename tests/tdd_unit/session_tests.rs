//! TDD 单元测试 — 会话管理器核心模块
//!
//! 测试会话创建、销毁、并发访问、双映射、顶号、僵尸清理

use std::net::SocketAddr;
use std::sync::Arc;

use rust_mmo_gate::session::session_mgr::SessionManager;
use rust_mmo_gate::session::session_struct::{MsgPriority, PendingMsg, SessionState};

#[tokio::test]
async fn test_session_manager_new_is_empty() {
    let mgr = SessionManager::new();
    assert_eq!(mgr.online_count(), 0);
}

#[tokio::test]
async fn test_create_session_increases_count() {
    let mgr = SessionManager::new();
    let addr: SocketAddr = "127.0.0.1:10001".parse().unwrap();
    let (sid, _rx) = mgr.create_session(addr, 1001).await;
    assert!(sid > 0, "session_id 应大于 0");
    assert_eq!(mgr.online_count(), 1);
}

#[tokio::test]
async fn test_get_session_by_id() {
    let mgr = SessionManager::new();
    let addr: SocketAddr = "127.0.0.1:10002".parse().unwrap();
    let (sid, _rx) = mgr.create_session(addr, 1002).await;
    let session = mgr.get_session(sid);
    assert!(session.is_some());
    assert_eq!(session.unwrap().player_uid(), 1002);
}

#[tokio::test]
async fn test_get_session_by_uid() {
    let mgr = SessionManager::new();
    let addr: SocketAddr = "127.0.0.1:10003".parse().unwrap();
    let (sid, _rx) = mgr.create_session(addr, 1003).await;
    let session = mgr.get_session_by_uid(1003);
    assert!(session.is_some());
    assert_eq!(session.unwrap().session_id, sid);
}

#[tokio::test]
async fn test_kick_session() {
    let mgr = SessionManager::new();
    let addr: SocketAddr = "127.0.0.1:10004".parse().unwrap();
    let (sid, _rx) = mgr.create_session(addr, 1004).await;
    assert_eq!(mgr.online_count(), 1);
    mgr.kick_session(sid, "test_kick").await;
    assert_eq!(mgr.online_count(), 0);
}

#[tokio::test]
async fn test_kick_nonexistent_session() {
    let mgr = SessionManager::new();
    // 不应panic
    mgr.kick_session(999999, "test").await;
}

#[tokio::test]
async fn test_duplicate_uid_kicks_old_session() {
    let mgr = SessionManager::new();
    let addr1: SocketAddr = "127.0.0.1:10005".parse().unwrap();
    let addr2: SocketAddr = "127.0.0.1:10006".parse().unwrap();
    let (sid1, _rx1) = mgr.create_session(addr1, 1005).await;
    let (sid2, _rx2) = mgr.create_session(addr2, 1005).await; // 相同uid
    assert_ne!(sid1, sid2);
    assert!(mgr.get_session(sid1).is_none(), "旧会话应被顶掉");
    assert!(mgr.get_session(sid2).is_some(), "新会话应存在");
    assert_eq!(mgr.online_count(), 1);
}

#[tokio::test]
async fn test_get_all_session_ids() {
    let mgr = SessionManager::new();
    let addr: SocketAddr = "127.0.0.1:10007".parse().unwrap();
    let (sid1, _rx1) = mgr.create_session(addr, 1007).await;
    let addr2: SocketAddr = "127.0.0.1:10008".parse().unwrap();
    let (sid2, _rx2) = mgr.create_session(addr2, 1008).await;
    let ids = mgr.get_all_session_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&sid1));
    assert!(ids.contains(&sid2));
}

#[tokio::test]
async fn test_clean_idle_sessions_noop_when_empty() {
    let mgr = SessionManager::new();
    mgr.clean_idle_sessions(45).await;
    // 不应 panic
}

#[tokio::test]
async fn test_concurrent_session_access() {
    let mgr = Arc::new(SessionManager::new());
    let mut handles = vec![];
    for i in 0..10 {
        let m = mgr.clone();
        handles.push(tokio::spawn(async move {
            let addr: SocketAddr = format!("127.0.0.1:{}", 20000 + i).parse().unwrap();
            let (_sid, _rx) = m.create_session(addr, 2000 + i as u64).await;
            let count = m.online_count();
            assert!(count > 0);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(mgr.online_count(), 10);
}

#[test]
fn test_msg_priority_ordering() {
    assert!(MsgPriority::High > MsgPriority::Normal);
    assert!(MsgPriority::Normal > MsgPriority::Low);
}

#[test]
fn test_session_state_transitions() {
    use rust_mmo_gate::session::session_struct::Session;
    use tokio::sync::mpsc;
    let (tx, _rx) = mpsc::unbounded_channel::<PendingMsg>();
    let session = Arc::new(Session::new(
        1,
        "127.0.0.1:9999".parse().unwrap(),
        100,
        tx,
    ));
    assert_eq!(session.state(), SessionState::Online);
    assert!(session.is_online());
    session.close();
    assert!(!session.is_online());
}
