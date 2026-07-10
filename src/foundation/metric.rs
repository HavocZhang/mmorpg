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
    register_int_counter, register_int_counter_vec, register_int_gauge, IntCounter, IntCounterVec,
    IntGauge, Opts, Registry,
};

/// 全局 Prometheus Registry（单例）
static REGISTRY: OnceLock<Registry> = OnceLock::new();

/// 全局指标集合
static METRICS: OnceLock<GateMetrics> = OnceLock::new();

/// 获取全局 Registry
pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| Registry::new())
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

        // 注册到 registry
        reg.register(Box::new(connections.clone())).ok();
        reg.register(Box::new(msgs_received.clone())).ok();
        reg.register(Box::new(msgs_sent.clone())).ok();
        reg.register(Box::new(decode_errors.clone())).ok();
        reg.register(Box::new(rate_limit_hits.clone())).ok();
        reg.register(Box::new(session_kicks.clone())).ok();
        reg.register(Box::new(cross_gate_msgs.clone())).ok();

        GateMetrics {
            connections,
            msgs_received,
            msgs_sent,
            decode_errors,
            rate_limit_hits,
            session_kicks,
            cross_gate_msgs,
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
