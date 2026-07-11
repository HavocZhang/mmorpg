//! # Rust MMO 百万在线网关集群 - 库入口
//!
//! 提供 pub 接口供集成测试和 benchmark 使用。
//! 网关零业务逻辑——scene/chat/combat 已迁移至 logic-lib crate。

pub mod admin;
pub mod cluster;
pub mod config;
pub mod crypto;
pub mod foundation;
pub mod grpc_router;
pub mod io_engine;
pub mod network;
pub mod protocol;
pub mod security;
pub mod session;

/// 游戏协议 proto 生成代码模块
///
/// 由 build.rs 从 proto/game.proto 编译生成
/// 包含所有上行/下行游戏消息和统一包装器 GameMessage
pub mod game_proto {
    include!(concat!(env!("OUT_DIR"), "/game.rs"));
}

use crate::config::AppConfig;

/// 网关全局共享上下文
pub struct GateContext {
    pub config: AppConfig,
}
