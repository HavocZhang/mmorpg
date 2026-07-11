//! TDD 单元测试 — 安全限流模块
//!
//! 测试 IP 黑名单、限流器、SecurityManager

use rust_mmo_gate::security::ip_blacklist::IpBlacklist;
use rust_mmo_gate::security::rate_limit::RateLimiter;
use rust_mmo_gate::security::SecurityManager;
use rust_mmo_gate::config::AppConfig;

use std::net::{IpAddr, Ipv4Addr};

fn make_ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(a, b, c, d))
}

#[test]
fn test_ip_blacklist_add_and_check() {
    let bl = IpBlacklist::new();
    let ip = make_ip(192, 168, 1, 100);
    assert!(!bl.is_blocked(&ip));
    bl.add(&ip);
    assert!(bl.is_blocked(&ip));
}

#[test]
fn test_ip_blacklist_remove() {
    let bl = IpBlacklist::new();
    let ip = make_ip(10, 0, 0, 1);
    bl.add(&ip);
    assert!(bl.is_blocked(&ip));
    bl.remove(&ip);
    assert!(!bl.is_blocked(&ip));
}

#[test]
fn test_ip_blacklist_not_blocked_by_default() {
    let bl = IpBlacklist::new();
    let ip = make_ip(127, 0, 0, 1);
    assert!(!bl.is_blocked(&ip));
}

#[test]
fn test_ip_blacklist_len() {
    let bl = IpBlacklist::new();
    assert_eq!(bl.len(), 0);
    bl.add(&make_ip(1, 1, 1, 1));
    bl.add(&make_ip(2, 2, 2, 2));
    assert_eq!(bl.len(), 2);
}

#[test]
fn test_rate_limiter_player_check() {
    let limiter = RateLimiter::new(100, 200, 10000);
    let player_id = 1001;
    // 首次请求应放行
    assert!(limiter.check_player_rate(player_id, false));
}

#[test]
fn test_rate_limiter_battle_vs_normal() {
    let limiter = RateLimiter::new(30, 80, 80000);
    let player_id = 1002;
    // 战斗模式有更高限流阈值
    for _ in 0..30 {
        assert!(limiter.check_player_rate(player_id, false));
    }
    // 普通模式第31次应该被限流
    assert!(!limiter.check_player_rate(player_id, false));
}

#[test]
fn test_rate_limiter_global_check() {
    let limiter = RateLimiter::new(100, 200, 5);
    // 全局限流检查
    for _ in 0..5 {
        assert!(limiter.check_global_rate());
    }
    assert!(!limiter.check_global_rate());
}

#[test]
fn test_security_manager_creation() {
    let config = AppConfig::load().unwrap();
    let mgr = SecurityManager::new(&config);
    let ip = make_ip(10, 20, 30, 40);
    assert!(!mgr.is_ip_blocked(&ip));
}

#[test]
fn test_security_manager_connect_rate() {
    let config = AppConfig::load().unwrap();
    let mgr = SecurityManager::new(&config);
    let ip = make_ip(172, 16, 0, 1);
    assert!(mgr.check_connect_rate(&ip));
}

#[test]
fn test_security_manager_player_rate() {
    let config = AppConfig::load().unwrap();
    let mgr = SecurityManager::new(&config);
    assert!(mgr.check_player_rate(9999, false));
}
