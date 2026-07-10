//! 基础底座模块：日志、监控指标、雪花ID、错误体系
//!
//! 阶段1核心：所有模块依赖的基础设施

pub mod error;
pub mod logger;
pub mod metric;
pub mod snowflake;

pub use error::GateError;
pub use snowflake::SnowflakeIdGen;
