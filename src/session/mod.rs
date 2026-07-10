//! Session 会话管理模块（核心）
//!
//! 阶段4核心：DashMap无锁双映射、会话生命周期、心跳巡检、资源释放

pub mod heartbeat_check;
pub mod session_mgr;
pub mod session_struct;

pub use session_mgr::SessionManager;
pub use session_struct::Session;
