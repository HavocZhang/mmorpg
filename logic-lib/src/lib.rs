//! MMO 游戏逻辑库
//!
//! 注意：此 crate 独立于网关，包含场景AOI、聊天、战斗系统。
//! 网关绝对禁止持有任何游戏业务状态。

pub mod scene;
pub mod chat;
pub mod combat;
pub mod db;
pub mod party;
