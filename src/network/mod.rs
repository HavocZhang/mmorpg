//! TCP/WebSocket 接入与握手鉴权模块
//!
//! 阶段3核心：异步监听、连接accept、版本校验、token校验、IP黑名单、会话初始化
//! v0.5: 新增 WebSocket 原生支持 (ws_listener.rs)

pub mod handshake;
pub mod tcp_listener;
pub mod ws_listener;
