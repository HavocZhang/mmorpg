//! TCP 接入与握手鉴权模块
//!
//! 阶段3核心：异步监听、连接accept、版本校验、token校验、IP黑名单、会话初始化
//!
//! v0.9: 移除 WebSocket 监听器，统一使用 TCP 接入 (浏览器通过 web-client/ws_proxy.js 桥接)

pub mod handshake;
pub mod tcp_listener;
