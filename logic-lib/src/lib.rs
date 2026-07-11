//! MMO 游戏逻辑库
//!
//! 注意：此 crate 独立于网关，包含场景AOI、聊天、战斗系统。
//! 网关绝对禁止持有任何游戏业务状态。

pub mod scene;
pub mod chat;
pub mod combat;
pub mod db;
pub mod party;

// 暴露游戏协议 proto 类型（由 rust-mmo-gate 的 build.rs 从 proto/game.proto 生成）
// logic_server 通过 `logic_lib::game_proto::PlayerStats` 等访问生成的类型
pub use rust_mmo_gate::game_proto;
