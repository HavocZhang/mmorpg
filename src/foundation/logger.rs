//! 日志初始化模块
//!
//! 基于 tracing 实现，支持 pretty/json 两种输出格式
//! - dev：pretty 格式，debug 级别
//! - prod：json 格式，info 级别

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// 初始化全局日志
///
/// # 参数
/// - `level`：日志级别字符串，如 "debug", "info", "warn", "error"
/// - `format`：输出格式，"pretty" 或 "json"
pub fn init(level: &str, format: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    match format {
        "json" => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .try_init();
        }
        _ => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .try_init();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::info;

    #[test]
    fn test_logger_init_pretty() {
        // 不应 panic
        init("debug", "pretty");
        info!("测试日志输出");
    }

    #[test]
    fn test_logger_init_json() {
        init("info", "json");
        info!("测试JSON日志输出");
    }
}
