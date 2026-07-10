//! # Rust MMO 百万在线网关集群 - 库入口
//!
//! 提供 pub 接口供集成测试和 benchmark 使用

pub mod admin;
pub mod cluster;
pub mod config;
pub mod crypto;
pub mod foundation;
pub mod grpc_router;
pub mod io_engine;
pub mod network;
pub mod protocol;
pub mod scene;
pub mod security;
pub mod session;

use crate::config::AppConfig;

/// 网关全局共享上下文
pub struct GateContext {
    pub config: AppConfig,
}
