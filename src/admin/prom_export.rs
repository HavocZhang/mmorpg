//! Prometheus 指标导出模块
//!
//! 暴露 /metrics 端点供 Prometheus 抓取

use crate::foundation::metric;

/// 生成 Prometheus 格式的指标文本
pub fn export() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let metric_families = metric::registry().gather();
    let mut buf = Vec::new();
    encoder.encode(&metric_families, &mut buf).ok();
    String::from_utf8(buf).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export() {
        // 初始化指标
        metric::metrics().msgs_received.inc();
        metric::metrics().connections.set(42);

        let output = export();
        assert!(output.contains("gate_msgs_received_total"));
        assert!(output.contains("gate_connections"));
    }
}
