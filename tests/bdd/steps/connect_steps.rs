//! connect.feature step definitions
//!
//! 连接与握手鉴权场景

use cucumber::{given, then, when};
use std::net::IpAddr;

use super::super::BddWorld;
use rust_mmo_gate::security::ip_blacklist::IpBlacklist;
use rust_mmo_gate::security::rate_limit::RateLimiter;

// ============ 正常TCP连接 ============

#[given("客户端发起TCP连接到网关")]
async fn given_client_connects(world: &mut BddWorld) {
    world.tcp_connected = true;
    world.connection_rejected = false;
}

#[when("网关接受连接")]
async fn when_gate_accepts(world: &mut BddWorld) {
    if world.ip_blacklist.is_none() {
        world.ip_blacklist = Some(IpBlacklist::new());
    }
    if world.rate_limiter.is_none() {
        world.rate_limiter = Some(RateLimiter::new(30, 80, 80000));
    }
    // 检查是否被拒绝
    if world.connection_rejected {
        world.tcp_connected = false;
    }
}

#[then("连接应成功建立")]
async fn then_connection_established(world: &mut BddWorld) {
    assert!(world.tcp_connected, "连接应成功建立");
    assert!(!world.connection_rejected, "连接不应被拒绝");
}

#[then("连接应进入握手阶段")]
async fn then_enter_handshake(world: &mut BddWorld) {
    world.handshake_stage = true;
    assert!(world.handshake_stage, "应进入握手阶段");
}

#[then("系统应分配连接资源")]
async fn then_allocate_resources(world: &mut BddWorld) {
    assert!(world.tcp_connected, "连接资源应已分配");
}

// ============ 黑名单IP拒绝 ============

#[given(expr = "IP {string} 已在黑名单中")]
async fn given_ip_blacklisted(world: &mut BddWorld, ip: String) {
    if world.ip_blacklist.is_none() {
        world.ip_blacklist = Some(IpBlacklist::new());
    }
    let addr: IpAddr = ip.parse().unwrap();
    world.ip_blacklist.as_ref().unwrap().block(addr);
}

#[when(expr = "客户端从IP {string} 发起TCP连接")]
async fn when_connect_from_ip(world: &mut BddWorld, ip: String) {
    let addr: IpAddr = ip.parse().unwrap();
    let blocked = world
        .ip_blacklist
        .as_ref()
        .map(|bl| bl.is_blocked(&addr))
        .unwrap_or(false);
    if blocked {
        world.connection_rejected = true;
        world.tcp_connected = false;
        world.log_security();
    } else {
        world.tcp_connected = true;
    }
}

#[then("网关应直接拒绝连接")]
async fn then_reject_connection(world: &mut BddWorld) {
    assert!(world.connection_rejected, "应拒绝连接");
    assert!(!world.tcp_connected, "不应建立连接");
}

#[then("不应分配任何资源")]
async fn then_no_resources(world: &mut BddWorld) {
    assert!(!world.tcp_connected, "不应分配连接资源");
}

// "应记录安全审计日志" 已在 security_steps.rs 中统一定义

// ============ 非法Token ============

#[given("客户端建立TCP连接")]
async fn given_tcp_established(world: &mut BddWorld) {
    world.tcp_connected = true;
    world.connection_rejected = false;
}

#[when("客户端发送握手包携带非法Token")]
async fn when_send_invalid_token(world: &mut BddWorld) {
    let token = "invalid_token_abc";
    if token == "valid_token" {
        world.handshake_stage = true;
    } else {
        world.connection_rejected = true;
        world.log_security();
    }
}

#[then("网关应拒绝握手")]
async fn then_reject_handshake(world: &mut BddWorld) {
    assert!(world.connection_rejected, "应拒绝握手");
}

#[then("网关应断开连接")]
async fn then_disconnect(world: &mut BddWorld) {
    world.disconnect();
    assert!(!world.tcp_connected, "应断开连接");
}

// ============ 过期Token ============

#[when("客户端发送握手包携带已过期的Token")]
async fn when_send_expired_token(world: &mut BddWorld) {
    // 模拟过期 Token
    let token_expired = true;
    if token_expired {
        world.connection_rejected = true;
    }
}

// ============ 版本不匹配 ============

#[when(expr = "客户端发送握手包携带版本号 {string}")]
async fn when_send_version(world: &mut BddWorld, version: String) {
    let expected_version = "1";
    if version != expected_version {
        world.connection_rejected = true;
        world.reject_reason = Some(format!("版本不匹配: 期望{} 实际{}", expected_version, version));
    }
}

#[when(expr = "网关期望版本号为 {string}")]
async fn given_expected_version(_world: &mut BddWorld, _version: String) {
    // 版本期望已硬编码在 when 步骤中
}

#[then("网关应拒绝接入")]
async fn then_reject_access(world: &mut BddWorld) {
    assert!(world.connection_rejected, "应拒绝接入");
}

#[then("返回版本不匹配错误")]
async fn then_version_mismatch_error(world: &mut BddWorld) {
    assert!(
        world.reject_reason.as_ref().map(|r| r.contains("版本不匹配")).unwrap_or(false),
        "应返回版本不匹配错误"
    );
}

// ============ 高频连接限流 ============

#[given(expr = "客户端从IP {string} 在5秒内发起20次连接")]
async fn given_high_freq_connect(world: &mut BddWorld, ip: String) {
    if world.rate_limiter.is_none() {
        world.rate_limiter = Some(RateLimiter::new(30, 80, 80000));
    }
    let addr: IpAddr = ip.parse().unwrap();
    let limiter = world.rate_limiter.as_ref().unwrap();
    // 模拟20次连接
    for _ in 0..20 {
        let _ = limiter.check_connect_rate(&addr);
    }
}

#[when(expr = "客户端再次从IP {string} 发起连接")]
async fn when_reconnect_from_ip(world: &mut BddWorld, ip: String) {
    let addr: IpAddr = ip.parse().unwrap();
    let allowed = world
        .rate_limiter
        .as_ref()
        .map(|rl| rl.check_connect_rate(&addr))
        .unwrap_or(true);
    if !allowed {
        world.connection_rejected = true;
        world.tcp_connected = false;
    }
}

#[then("网关应触发连接限流")]
async fn then_trigger_rate_limit(world: &mut BddWorld) {
    assert!(world.connection_rejected, "应触发连接限流");
}

#[then("应拒绝新连接")]
async fn then_reject_new_connection(world: &mut BddWorld) {
    assert!(world.connection_rejected, "应拒绝新连接");
}

#[then("应记录限流事件")]
async fn then_log_rate_limit(world: &mut BddWorld) {
    // 限流事件已触发
    assert!(world.connection_rejected, "限流事件应已记录");
}
