//! ECS 资源定义
//!
//! 资源是全局共享的单例数据，不绑定到特定实体。

use bevy::prelude::*;
use std::collections::HashMap;

/// 玩家自身状态
#[derive(Resource, Default)]
pub struct PlayerState {
    pub uid: u64,
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub level: u32,
    pub exp: u32,
    pub max_exp: u32,
    pub gold: u32,
    pub atk: i32,
    pub def: i32,
    pub x: f32,
    pub y: f32,
    pub logged_in: bool,
}

/// 实体管理器 (服务端同步的实体缓存)
#[derive(Resource, Default)]
pub struct EntityManager {
    /// entity_id -> EntityInfo
    pub entities: HashMap<u64, EntityInfo>,
}

/// 实体信息
#[derive(Clone, Debug)]
pub struct EntityInfo {
    pub entity_id: u64,
    pub def_id: u32,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub max_hp: i32,
    pub level: u32,
    pub entity_type: String,
}

impl EntityInfo {
    /// 推断实体类型
    pub fn is_npc(&self) -> bool {
        !self.entity_type.is_empty()
    }
}

/// 其他玩家管理器
#[derive(Resource, Default)]
pub struct OtherPlayerManager {
    /// uid -> PlayerInfo
    pub players: HashMap<u64, OtherPlayerInfo>,
}

/// 其他玩家信息
#[derive(Clone, Debug)]
pub struct OtherPlayerInfo {
    pub uid: u64,
    pub name: String,
    pub x: f32,
    pub y: f32,
}

/// 游戏配置 (从服务端拉取)
#[derive(Resource, Default)]
pub struct GameConfig {
    pub loaded: bool,
    pub items: Vec<serde_json::Value>,
    #[allow(dead_code)]
    pub quests: Vec<serde_json::Value>,
}

/// 输入状态 (移动节流)
#[derive(Resource, Default)]
pub struct InputState {
    pub last_move_time: u64,
}

/// 网络连接状态
#[derive(Resource, Default)]
pub struct ConnectionState {
    pub connected: bool,
    pub connecting: bool,
}

/// UI 文本缓存 (用于更新 HUD)
#[derive(Resource, Default)]
pub struct UiTextCache {
    pub hud_text: String,
}
