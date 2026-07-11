//! Prometheus 监控指标模块
//!
//! 网关核心监控指标：
//! - 在线连接数
//! - 消息收发速率
//! - 协议解码错误数
//! - 限流触发次数
//! - 会话生命周期事件

use std::sync::OnceLock;

use prometheus::{
    register_int_counter, register_int_counter_vec, register_int_gauge,
    register_histogram, IntCounter, IntCounterVec, IntGauge, Histogram, Opts, Registry,
};

/// 全局 Prometheus Registry（单例）
static REGISTRY: OnceLock<Registry> = OnceLock::new();

/// 全局指标集合
static METRICS: OnceLock<GateMetrics> = OnceLock::new();

/// 获取全局 Registry
pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::new)
}

/// 获取全局指标集合
pub fn metrics() -> &'static GateMetrics {
    METRICS.get_or_init(|| {
        let reg = registry();
        GateMetrics::new(reg)
    })
}

/// 网关核心监控指标
pub struct GateMetrics {
    /// 当前在线连接数
    pub connections: IntGauge,
    /// 总接收消息数
    pub msgs_received: IntCounter,
    /// 总发送消息数
    pub msgs_sent: IntCounter,
    /// 协议解码错误数
    pub decode_errors: IntCounterVec,
    /// 限流触发次数
    pub rate_limit_hits: IntCounterVec,
    /// 会话踢出次数
    pub session_kicks: IntCounterVec,
    /// 跨网关消息数
    pub cross_gate_msgs: IntCounter,
    // --- 生产级扩展指标 ---
    /// 合包总包数
    pub merge_total_packets: IntCounter,
    /// 合包总 flush 次数
    pub merge_total_flushes: IntCounter,
    /// 合包总发送字节数
    pub merge_total_bytes: IntCounter,
    /// 进程内存使用 (bytes)
    pub process_memory_bytes: IntGauge,
    /// 网关运行时间 (秒)
    pub uptime_seconds: IntGauge,
    /// 消息处理延迟分布 (毫秒)
    pub msg_latency_ms: Histogram,
    /// 活跃 Session 数
    pub active_sessions: IntGauge,
    /// gRPC 上游请求计数
    pub grpc_upstream_requests: IntCounterVec,
    /// gRPC 上游错误计数
    pub grpc_upstream_errors: IntCounterVec,
    /// IP 黑名单数量
    pub blacklist_count: IntGauge,
    /// 每秒接收消息速率 (由采集端计算)
    pub msgs_received_rate: IntGauge,
    /// 每秒发送消息速率 (由采集端计算)
    pub msgs_sent_rate: IntGauge,
}

impl GateMetrics {
    fn new(reg: &Registry) -> Self {
        let connections = register_int_gauge!(Opts::new(
            "gate_connections",
            "当前在线连接数"
        ))
        .unwrap();

        let msgs_received = register_int_counter!(Opts::new(
            "gate_msgs_received_total",
            "总接收消息数"
        ))
        .unwrap();

        let msgs_sent = register_int_counter!(Opts::new(
            "gate_msgs_sent_total",
            "总发送消息数"
        ))
        .unwrap();

        let decode_errors = register_int_counter_vec!(
            Opts::new("gate_decode_errors_total", "协议解码错误数"),
            &["type"]
        )
        .unwrap();

        let rate_limit_hits = register_int_counter_vec!(
            Opts::new("gate_rate_limit_hits_total", "限流触发次数"),
            &["scope"]
        )
        .unwrap();

        let session_kicks = register_int_counter_vec!(
            Opts::new("gate_session_kicks_total", "会话踢出次数"),
            &["reason"]
        )
        .unwrap();

        let cross_gate_msgs = register_int_counter!(Opts::new(
            "gate_cross_gate_msgs_total",
            "跨网关消息数"
        ))
        .unwrap();

        // --- 生产级扩展指标 ---
        let merge_total_packets = register_int_counter!(Opts::new(
            "gate_merge_total_packets",
            "合包总包数"
        ))
        .unwrap();

        let merge_total_flushes = register_int_counter!(Opts::new(
            "gate_merge_total_flushes",
            "合包总flush次数"
        ))
        .unwrap();

        let merge_total_bytes = register_int_counter!(Opts::new(
            "gate_merge_total_bytes",
            "合包总发送字节数"
        ))
        .unwrap();

        let process_memory_bytes = register_int_gauge!(Opts::new(
            "gate_process_memory_bytes",
            "进程内存使用量(bytes)"
        ))
        .unwrap();

        let uptime_seconds = register_int_gauge!(Opts::new(
            "gate_uptime_seconds",
            "网关运行时间(秒)"
        ))
        .unwrap();

        let msg_latency_ms = register_histogram!(
            "gate_msg_latency_ms",
            "消息处理延迟(毫秒)",
            vec![1.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 150.0, 200.0, 500.0]
        )
        .unwrap();

        let active_sessions = register_int_gauge!(Opts::new(
            "gate_active_sessions",
            "活跃Session数"
        ))
        .unwrap();

        let grpc_upstream_requests = register_int_counter_vec!(
            Opts::new("gate_grpc_upstream_requests_total", "gRPC上游请求总数"),
            &["endpoint"]
        )
        .unwrap();

        let grpc_upstream_errors = register_int_counter_vec!(
            Opts::new("gate_grpc_upstream_errors_total", "gRPC上游错误总数"),
            &["endpoint", "error_type"]
        )
        .unwrap();

        let blacklist_count = register_int_gauge!(Opts::new(
            "gate_blacklist_count",
            "IP黑名单数量"
        ))
        .unwrap();

        let msgs_received_rate = register_int_gauge!(Opts::new(
            "gate_msgs_received_rate",
            "每秒接收消息数"
        ))
        .unwrap();

        let msgs_sent_rate = register_int_gauge!(Opts::new(
            "gate_msgs_sent_rate",
            "每秒发送消息数"
        ))
        .unwrap();

        // 注册到 registry
        reg.register(Box::new(connections.clone())).ok();
        reg.register(Box::new(msgs_received.clone())).ok();
        reg.register(Box::new(msgs_sent.clone())).ok();
        reg.register(Box::new(decode_errors.clone())).ok();
        reg.register(Box::new(rate_limit_hits.clone())).ok();
        reg.register(Box::new(session_kicks.clone())).ok();
        reg.register(Box::new(cross_gate_msgs.clone())).ok();
        reg.register(Box::new(merge_total_packets.clone())).ok();
        reg.register(Box::new(merge_total_flushes.clone())).ok();
        reg.register(Box::new(merge_total_bytes.clone())).ok();
        reg.register(Box::new(process_memory_bytes.clone())).ok();
        reg.register(Box::new(uptime_seconds.clone())).ok();
        reg.register(Box::new(msg_latency_ms.clone())).ok();
        reg.register(Box::new(active_sessions.clone())).ok();
        reg.register(Box::new(grpc_upstream_requests.clone())).ok();
        reg.register(Box::new(grpc_upstream_errors.clone())).ok();
        reg.register(Box::new(blacklist_count.clone())).ok();
        reg.register(Box::new(msgs_received_rate.clone())).ok();
        reg.register(Box::new(msgs_sent_rate.clone())).ok();

        GateMetrics {
            connections,
            msgs_received,
            msgs_sent,
            decode_errors,
            rate_limit_hits,
            session_kicks,
            cross_gate_msgs,
            merge_total_packets,
            merge_total_flushes,
            merge_total_bytes,
            process_memory_bytes,
            uptime_seconds,
            msg_latency_ms,
            active_sessions,
            grpc_upstream_requests,
            grpc_upstream_errors,
            blacklist_count,
            msgs_received_rate,
            msgs_sent_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_init() {
        let m = metrics();
        m.msgs_received.inc();
        m.connections.set(42);
        m.decode_errors.with_label_values(&["crc"]).inc();
        m.rate_limit_hits.with_label_values(&["player"]).inc();
        m.session_kicks.with_label_values(&["heartbeat"]).inc();

        assert_eq!(m.connections.get(), 42);
    }
}
