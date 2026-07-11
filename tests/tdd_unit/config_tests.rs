//! TDD 单元测试 — 配置中心模块
//!
//! 测试 AppConfig 加载、环境变量解析、默认值、边界条件

use rust_mmo_gate::config::AppConfig;

/// 测试默认配置加载（使用 .env.dev）
#[test]
fn test_config_load_success() {
    let config = AppConfig::load();
    assert!(config.is_ok(), "配置加载应成功: {:?}", config.err());
    let config = config.unwrap();
    assert_eq!(config.app.env, "dev");
}

/// 测试网关基本配置
#[test]
fn test_gate_config_values() {
    let config = AppConfig::load().unwrap();
    assert_eq!(config.gate.tcp_port, 7888);
    assert_eq!(config.gate.http_port, 9090);
    assert_eq!(config.gate.node_id, 1);
    assert!(!config.gate.node_name.is_empty());
}

/// 测试会话配置
#[test]
fn test_session_config() {
    let config = AppConfig::load().unwrap();
    assert_eq!(config.session.heartbeat_timeout_secs, 45);
    assert_eq!(config.session.heartbeat_check_interval_secs, 10);
}

/// 测试加密配置
#[test]
fn test_crypto_config() {
    let config = AppConfig::load().unwrap();
    assert_eq!(config.crypto.aes_key.len(), 64, "AES key 应为 64 hex 字符");
}

/// 测试限流配置
#[test]
fn test_rate_limit_config() {
    let config = AppConfig::load().unwrap();
    assert!(config.rate_limit.player_per_sec > 0);
    assert!(config.rate_limit.global_per_sec > 0);
}

/// 测试 gRPC 端点配置
#[test]
fn test_grpc_endpoints_config() {
    let config = AppConfig::load().unwrap();
    let endpoints: Vec<&str> = config.grpc.logic_endpoints.split(',').filter(|s| !s.is_empty()).collect();
    assert!(!endpoints.is_empty(), "应有至少一个 gRPC 端点");
}

/// 测试 IO 配置
#[test]
fn test_io_config() {
    let config = AppConfig::load().unwrap();
    assert!(config.io.packet_merge_window_ms > 0);
}
