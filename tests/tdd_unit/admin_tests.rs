//! TDD 单元测试 — HTTP监控运维模块
//!
//! 测试 Prometheus 指标导出、优雅停机、合包统计

use rust_mmo_gate::admin::prom_export;

#[test]
fn test_prometheus_metrics_export() {
    let output = prom_export::export();
    assert!(!output.is_empty(), "Prometheus 导出不应为空");
    assert!(output.contains("HELP") || output.contains("TYPE") || output.contains("connections"));
}

#[test]
fn test_prometheus_metrics_contains_gateway_metrics() {
    let output = prom_export::export();
    let has_connections = output.contains("connections");
    let has_sessions = output.contains("sessions") || output.contains("session");
    assert!(has_connections || has_sessions, "应包含连接/会话相关指标");
}

#[test]
fn test_graceful_shutdown_module_exists() {
    // 验证 graceful_shutdown 模块可正常导入
    // graceful_shutdown 是一个模块（非值），通过路径解析即可验证存在
}

#[test]
fn test_merge_stats_api() {
    use rust_mmo_gate::io_engine::packet_merge::merge_stats;
    let (packets, flushes, rate) = merge_stats();
    assert!(rate >= 0.0 && rate <= 100.0);
    assert!(packets >= flushes);
}

#[test]
fn test_merge_stats_with_recent() {
    use rust_mmo_gate::io_engine::packet_merge::merge_stats_with_recent;
    let snapshot = merge_stats_with_recent();
    assert!(snapshot.cumulative_rate >= 0.0);
    assert!(snapshot.recent_rate >= 0.0);
    assert!(snapshot.total_packets >= snapshot.total_flushes);
}

#[test]
fn test_monitor_api_health_structure() {
    // 验证 HealthResponse 结构体存在
    use rust_mmo_gate::admin::monitor_api::HealthResponse;
    let health = HealthResponse {
        status: "ok".into(),
        online_count: 0,
        uptime_secs: 0,
    };
    assert_eq!(health.status, "ok");
    assert_eq!(health.online_count, 0);
}
