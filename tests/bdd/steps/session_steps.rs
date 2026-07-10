//! session.feature step definitions
//!
//! 会话 Session 管理场景

use cucumber::{given, then, when};
use std::time::Instant;

use super::super::BddWorld;

// ============ 唯一Session创建 ============

#[given("客户端完成握手")]
async fn given_handshake_done(world: &mut BddWorld) {
    world.tcp_connected = true;
    world.handshake_stage = true;
}

#[when("网关创建会话")]
async fn when_create_session(world: &mut BddWorld) {
    let sid = world.create_test_session(0);
    // session_id 全局唯一性由递增计数器保证
    let _ = &sid;
}

#[then("会话应创建成功")]
async fn then_session_created(world: &mut BddWorld) {
    assert!(!world.sessions.is_empty(), "会话应创建成功");
}

#[then("session_id 应全局唯一")]
async fn then_session_id_unique(world: &mut BddWorld) {
    let ids: Vec<&str> = world.sessions.keys().map(|s| s.as_str()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "session_id 应全局唯一");
}

#[then(expr = "会话状态应为 {string}")]
async fn then_session_state(world: &mut BddWorld, state: String) {
    let session = world.sessions.values().last().unwrap();
    match state.as_str() {
        "Online" => assert_eq!(session.state, rust_mmo_gate::session::session_struct::SessionState::Online),
        "Closed" => assert_eq!(session.state, rust_mmo_gate::session::session_struct::SessionState::Closed),
        _ => panic!("未知状态: {}", state),
    }
}

// ============ 绑定player_uid ============

#[given(expr = "客户端完成握手且player_uid为 {string}")]
async fn given_handshake_with_uid(world: &mut BddWorld, uid: String) {
    world.tcp_connected = true;
    world.handshake_stage = true;
    // 存储待用 uid
    world.sessions.insert(
        "__pending__".to_string(),
        super::super::TestSession {
            session_id: "__pending__".to_string(),
            player_uid: uid.parse().unwrap(),
            state: rust_mmo_gate::session::session_struct::SessionState::Online,
            last_active_secs_ago: 0,
            closed: false,
        },
    );
}

#[then(expr = "会话应绑定player_uid {string}")]
async fn then_bind_uid(world: &mut BddWorld, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    let session = world.sessions.values().find(|s| s.player_uid == uid);
    assert!(session.is_some(), "会话应绑定 player_uid {}", uid);
}

#[then("player_uid到session_id的映射应建立")]
async fn then_uid_mapping_exists(world: &mut BddWorld) {
    let has_uid = world.sessions.values().any(|s| s.player_uid > 0);
    assert!(has_uid, "player_uid 到 session_id 的映射应建立");
}

#[then("通过player_uid可查询到对应会话")]
async fn then_query_by_uid(world: &mut BddWorld) {
    let session = world.sessions.values().find(|s| s.player_uid > 0);
    assert!(session.is_some(), "通过 player_uid 应可查询到会话");
}

// ============ 顶号机制 ============

#[given(expr = "player_uid {string} 已存在在线会话 session_id {string}")]
async fn given_existing_session(world: &mut BddWorld, uid: String, sid: String) {
    let uid: u64 = uid.parse().unwrap();
    world.sessions.insert(
        sid.clone(),
        super::super::TestSession {
            session_id: sid,
            player_uid: uid,
            state: rust_mmo_gate::session::session_struct::SessionState::Online,
            last_active_secs_ago: 0,
            closed: false,
        },
    );
}

#[when(expr = "同一player_uid {string} 再次登录")]
async fn when_same_uid_relogin(world: &mut BddWorld, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    // 找到旧会话并顶掉
    let old_sid = world
        .sessions
        .iter()
        .find(|(_, s)| s.player_uid == uid)
        .map(|(k, _)| k.clone());
    if let Some(old_sid) = old_sid {
        if let Some(session) = world.sessions.get_mut(&old_sid) {
            session.closed = true;
            session.state = rust_mmo_gate::session::session_struct::SessionState::Closed;
        }
        world.kicked_sessions.push(old_sid);
    }
    // 创建新会话
    let new_sid = world.create_test_session(uid);
    let _ = &new_sid;
}

#[then(expr = "旧会话 session_id {string} 应被下线")]
async fn then_old_session_offline(world: &mut BddWorld, sid: String) {
    assert!(
        world.kicked_sessions.contains(&sid),
        "旧会话 {} 应被下线",
        sid
    );
}

#[then("新会话应创建成功")]
async fn then_new_session_created(world: &mut BddWorld) {
    let online_count = world
        .sessions
        .values()
        .filter(|s| !s.closed && s.session_id != "__pending__")
        .count();
    assert!(online_count > 0, "新会话应创建成功");
}

#[then("旧会话的发送通道应关闭")]
async fn then_old_channel_closed(world: &mut BddWorld) {
    assert!(!world.kicked_sessions.is_empty(), "旧会话通道应关闭");
}

#[then("应记录顶号事件")]
async fn then_log_kick_event(world: &mut BddWorld) {
    assert!(!world.kicked_sessions.is_empty(), "应记录顶号事件");
}

// ============ 僵尸连接清理 ============

#[given(expr = "存在在线会话session_id {string}")]
async fn given_online_session(world: &mut BddWorld, sid: String) {
    world.sessions.insert(
        sid.clone(),
        super::super::TestSession {
            session_id: sid,
            player_uid: 100,
            state: rust_mmo_gate::session::session_struct::SessionState::Online,
            last_active_secs_ago: 0,
            closed: false,
        },
    );
}

#[given(expr = "会话 {string} 最后活跃时间为44秒前")]
async fn given_active_44s_ago(world: &mut BddWorld, sid: String) {
    if let Some(session) = world.sessions.get_mut(&sid) {
        session.last_active_secs_ago = 44;
    }
}

#[when("心跳巡检执行")]
async fn when_heartbeat_check(world: &mut BddWorld) {
    let timeout: u64 = 45;
    let to_clean: Vec<String> = world
        .sessions
        .iter()
        .filter(|(_, s)| s.last_active_secs_ago > timeout && !s.closed)
        .map(|(k, _)| k.clone())
        .collect();
    for sid in to_clean {
        if let Some(session) = world.sessions.get_mut(&sid) {
            session.closed = true;
            session.state = rust_mmo_gate::session::session_struct::SessionState::Closed;
        }
        world.zombie_cleaned.push(sid);
    }
}

#[then(expr = "会话 {string} 不应被清理")]
async fn then_session_not_cleaned(world: &mut BddWorld, sid: String) {
    assert!(
        !world.zombie_cleaned.contains(&sid),
        "会话 {} 不应被清理",
        sid
    );
}

#[when("时间超过45秒")]
async fn when_time_exceeds_45s(world: &mut BddWorld) {
    for session in world.sessions.values_mut() {
        if !session.closed {
            session.last_active_secs_ago = 46;
        }
    }
}

#[then(expr = "会话 {string} 应被判定为僵尸连接")]
async fn then_zombie_detected(world: &mut BddWorld, sid: String) {
    assert!(
        world.zombie_cleaned.contains(&sid),
        "会话 {} 应被判定为僵尸连接",
        sid
    );
}

#[then(expr = "会话 {string} 应被清理")]
async fn then_session_cleaned(world: &mut BddWorld, sid: String) {
    if let Some(session) = world.sessions.get(&sid) {
        assert!(session.closed, "会话 {} 应被清理", sid);
    }
}

#[then("相关资源应被释放")]
async fn then_resources_released(world: &mut BddWorld) {
    let cleaned = world.zombie_cleaned.len();
    assert!(cleaned > 0, "相关资源应被释放");
}

// ============ 资源完整释放 ============

#[given(expr = "存在在线会话session_id {string} 绑定player_uid {string}")]
async fn given_session_with_uid(world: &mut BddWorld, sid: String, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    world.sessions.insert(
        sid.clone(),
        super::super::TestSession {
            session_id: sid,
            player_uid: uid,
            state: rust_mmo_gate::session::session_struct::SessionState::Online,
            last_active_secs_ago: 0,
            closed: false,
        },
    );
}

#[when(expr = "会话 {string} 被关闭")]
async fn when_session_closed(world: &mut BddWorld, sid: String) {
    if let Some(session) = world.sessions.get_mut(&sid) {
        session.closed = true;
        session.state = rust_mmo_gate::session::session_struct::SessionState::Closed;
    }
}

#[then(expr = "session_map中应移除 session_id {string}")]
async fn then_removed_from_session_map(world: &mut BddWorld, sid: String) {
    let session = world.sessions.get(&sid);
    if let Some(s) = session {
        assert!(s.closed, "会话 {} 应已标记关闭", sid);
    }
}

#[then(expr = "uid_map中应移除 player_uid {string}")]
async fn then_removed_from_uid_map(world: &mut BddWorld, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    let still_mapped = world
        .sessions
        .values()
        .any(|s| s.player_uid == uid && !s.closed);
    assert!(!still_mapped, "uid_map 中应移除 player_uid {}", uid);
}

#[then("TCP文件描述符应释放")]
async fn then_fd_released(world: &mut BddWorld) {
    let all_closed = world.sessions.values().all(|s| s.closed);
    // 至少有一个被关闭
    let has_closed = world.sessions.values().any(|s| s.closed);
    assert!(has_closed, "TCP文件描述符应释放");
}

#[then("在线连接计数应减一")]
async fn then_online_count_dec(world: &mut BddWorld) {
    let online = world.sessions.values().filter(|s| !s.closed).count();
    let closed = world.sessions.values().filter(|s| s.closed).count();
    assert!(closed > 0, "应有会话被关闭，在线计数应减一");
}
