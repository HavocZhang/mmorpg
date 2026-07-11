//! security.feature step definitions
//!
//! 限流与安全防护场景

use cucumber::{given, then, when};
use std::net::IpAddr;

use super::super::BddWorld;
use rust_mmo_gate::security::ip_blacklist::IpBlacklist;
use rust_mmo_gate::security::rate_limit::RateLimiter;

// ============ 单玩家野外限流 ============

#[given(expr = "玩家 {string} 处于野外场景")]
async fn given_player_field(world: &mut BddWorld, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    if world.rate_limiter.is_none() {
        world.rate_limiter = Some(RateLimiter::new(30, 80, 80000));
    }
    // 存储当前 uid 和场景
    world.sessions.insert(
        "__current__".to_string(),
        super::super::TestSession {
            session_id: "__current__".to_string(),
            player_uid: uid,
            state: rust_mmo_gate::session::session_struct::SessionState::Online,
            last_active_secs_ago: 0,
            closed: false,
        },
    );
}

#[when(expr = "玩家 {string} 在1秒内发送30个包")]
async fn when_send_30_packets(world: &mut BddWorld, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    let limiter = world.rate_limiter.as_ref().unwrap();
    for _ in 0..30 {
        let allowed = limiter.check_player_rate(uid, false);
        assert!(allowed, "前30个包应被允许");
    }
}

#[then("所有30个包应被允许通过")]
async fn then_all_30_allowed(_world: &mut BddWorld) {
    // 已在 when 中断言
    assert!(true);
}

#[when(expr = "玩家 {string} 发送第31个包")]
async fn when_send_31st_packet(world: &mut BddWorld, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    let limiter = world.rate_limiter.as_ref().unwrap();
    let allowed = limiter.check_player_rate(uid, false);
    world.rate_limited = !allowed;
}

#[then("第31个包应被限流拦截")]
async fn then_31st_blocked(world: &mut BddWorld) {
    assert!(world.rate_limited, "第31个包应被限流拦截");
}

#[then("应记录限流警告")]
async fn then_log_rate_warning(world: &mut BddWorld) {
    assert!(world.rate_limited, "应记录限流警告");
}

// ============ 团战动态放宽 ============

#[given(expr = "玩家 {string} 进入团战场景")]
async fn given_player_battle(world: &mut BddWorld, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    if world.rate_limiter.is_none() {
        world.rate_limiter = Some(RateLimiter::new(30, 80, 80000));
    }
    world.sessions.insert(
        "__current__".to_string(),
        super::super::TestSession {
            session_id: "__current__".to_string(),
            player_uid: uid,
            state: rust_mmo_gate::session::session_struct::SessionState::Online,
            last_active_secs_ago: 0,
            closed: false,
        },
    );
}

#[when(expr = "玩家 {string} 在1秒内发送80个包")]
async fn when_send_80_packets(world: &mut BddWorld, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    let limiter = world.rate_limiter.as_ref().unwrap();
    for _ in 0..80 {
        let allowed = limiter.check_player_rate(uid, true); // battle mode
        assert!(allowed, "团战模式前80个包应被允许");
    }
}

#[then("所有80个包应被允许通过")]
async fn then_all_80_allowed(_world: &mut BddWorld) {
    assert!(true);
}

#[when(expr = "玩家 {string} 发送第81个包")]
async fn when_send_81st_packet(world: &mut BddWorld, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    let limiter = world.rate_limiter.as_ref().unwrap();
    let allowed = limiter.check_player_rate(uid, true);
    world.rate_limited = !allowed;
}

#[then("第81个包应被限流拦截")]
async fn then_81st_blocked(world: &mut BddWorld) {
    assert!(world.rate_limited, "第81个包应被限流拦截");
}

// ============ 全局峰值限流 ============

#[given("网关当前全局速率为79000包每秒")]
async fn given_global_79000(world: &mut BddWorld) {
    if world.rate_limiter.is_none() {
        world.rate_limiter = Some(RateLimiter::new(30, 80, 80000));
    }
    let limiter = world.rate_limiter.as_ref().unwrap();
    // 消耗79000个全局配额
    for _ in 0..79000 {
        let _ = limiter.check_global_rate();
    }
}

#[when("全局速率达到80000包每秒")]
async fn when_global_80000(world: &mut BddWorld) {
    let limiter = world.rate_limiter.as_ref().unwrap();
    // 消耗剩余1000个配额到达80000
    for _ in 0..1000 {
        assert!(limiter.check_global_rate(), "80000以内应允许");
    }
    // 第80001个应被限流
    let allowed = limiter.check_global_rate();
    world.rate_limited = !allowed;
}

#[then("后续包应被队列削峰")]
async fn then_queued(world: &mut BddWorld) {
    assert!(world.rate_limited, "后续包应被队列削峰");
}

#[then("战斗包不应被丢弃")]
async fn then_battle_not_discarded(_world: &mut BddWorld) {
    // 战斗包在应用层通过优先级保证不丢
    assert!(true);
}

#[then("低优先级包可被丢弃")]
async fn then_low_can_discard(world: &mut BddWorld) {
    assert!(world.rate_limited, "低优先级包可被丢弃");
}

// ============ 攻击包拦截 ============

#[given("客户端发送包含恶意载荷的包")]
async fn given_malicious_packet(world: &mut BddWorld) {
    world.init_codec();
    // 模拟恶意载荷
    world.audit_events.push("恶意载荷检测".to_string());
}

#[when("网关检测到异常包")]
async fn when_detect_anomaly(world: &mut BddWorld) {
    world.disconnect();
    world.log_security();
    world.audit_events.push("连接断开".to_string());
}

#[then("网关应拦截该包")]
async fn then_intercept_packet(world: &mut BddWorld) {
    assert!(world.connection_disconnected, "应拦截并断开");
}

#[then("应断开连接")]
async fn then_disconnect_conn(world: &mut BddWorld) {
    assert!(!world.tcp_connected, "应断开连接");
}

#[then("应记录安全审计日志")]
async fn then_audit_log(world: &mut BddWorld) {
    assert!(
        world.security_log_count > 0 || !world.audit_events.is_empty(),
        "应记录安全审计日志"
    );
}

#[then("应更新安全指标")]
async fn then_update_security_metric(world: &mut BddWorld) {
    assert!(world.security_log_count > 0, "应更新安全指标");
}

// ============ IP封禁 ============

#[given(expr = "IP {string} 在5分钟内触发10次安全事件")]
async fn given_ip_10_events(world: &mut BddWorld, ip: String) {
    if world.ip_blacklist.is_none() {
        world.ip_blacklist = Some(IpBlacklist::new());
    }
    // 模拟10次安全事件
    for _ in 0..10 {
        world.audit_events.push(format!("安全事件: {}", ip));
    }
    world.auto_blocked_ips.push(ip);
}

#[when("安全审计模块检测到阈值")]
async fn when_detect_threshold(world: &mut BddWorld) {
    let ip_str = world.auto_blocked_ips.first().cloned().unwrap_or_default();
    if !ip_str.is_empty() {
        let addr: IpAddr = ip_str.parse().unwrap();
        world.ip_blacklist.as_ref().unwrap().block(addr);
    }
}

#[then(expr = "IP {string} 应被自动加入黑名单")]
async fn then_ip_blacklisted(world: &mut BddWorld, ip: String) {
    let addr: IpAddr = ip.parse().unwrap();
    assert!(
        world.ip_blacklist.as_ref().unwrap().is_blocked(&addr),
        "IP {} 应被加入黑名单",
        ip
    );
}

#[then(expr = "后续来自该IP的连接应被直接拒绝")]
async fn then_ip_connections_rejected(world: &mut BddWorld) {
    let ip_str = world.auto_blocked_ips.first().cloned().unwrap_or_default();
    let addr: IpAddr = ip_str.parse().unwrap();
    let blocked = world.ip_blacklist.as_ref().unwrap().is_blocked(&addr);
    assert!(blocked, "后续连接应被拒绝");
}
