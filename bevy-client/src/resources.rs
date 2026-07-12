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
    /// 是否为 NPC (entity_type 非空且不为 "mob")
    pub fn is_npc(&self) -> bool {
        !self.entity_type.is_empty() && self.entity_type != "mob"
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

/// 背包物品
#[derive(Clone, Debug)]
pub struct InventoryItem {
    pub item_id: u32,
    pub count: u32,
    pub name: String,
    pub item_type: String,
    pub icon: String,
}

/// 装备槽
#[derive(Clone, Debug, Default)]
pub struct EquipmentSlot {
    pub item_id: u32,
    pub name: String,
    pub icon: String,
    pub enhance_level: u32,
    pub empty: bool,
}

/// 装备数据
#[derive(Clone, Debug, Default)]
pub struct EquipmentData {
    pub weapon: EquipmentSlot,
    pub armor: EquipmentSlot,
    pub accessory: EquipmentSlot,
}

/// 任务条目
#[derive(Clone, Debug)]
pub struct QuestEntry {
    pub quest_id: u32,
    pub name: String,
    pub progress: u32,
    pub target: u32,
    pub desc: String,
    pub completed: bool,
}

/// 掉落物品
#[derive(Clone, Debug)]
pub struct DropItem {
    pub drop_id: u64,
    pub item_id: u32,
    pub count: u32,
    pub x: f32,
    pub y: f32,
}

/// NPC 对话信息
#[derive(Clone, Debug, Default)]
pub struct NpcDialogInfo {
    pub npc_id: u32,
    pub name: String,
    pub dialog: String,
    pub options: Vec<NpcDialogOption>,
}

/// NPC 对话选项
#[derive(Clone, Debug)]
pub struct NpcDialogOption {
    pub label: String,
    pub action: DialogAction,
}

/// 对话选项动作
#[derive(Clone, Debug, PartialEq)]
pub enum DialogAction {
    /// 接受任务
    AcceptQuest(u32),
    /// 完成任务
    CompleteQuest(u32),
    /// 打开商店
    OpenShop,
    /// 关闭对话
    Close,
    /// 无动作 (纯文本)
    None,
}

/// 玩家背包
#[derive(Resource, Default)]
pub struct Inventory {
    pub items: Vec<InventoryItem>,
}

/// 玩家装备
#[derive(Resource, Default)]
pub struct Equipment {
    pub data: EquipmentData,
}

/// 玩家任务列表
#[derive(Resource, Default)]
pub struct QuestLog {
    pub quests: Vec<QuestEntry>,
}

/// 掉落物品管理器
#[derive(Resource, Default)]
pub struct DropManager {
    pub drops: HashMap<u64, DropItem>,
}

/// NPC 对话状态
#[derive(Resource, Default)]
pub struct NpcDialogState {
    pub dialog: Option<NpcDialogInfo>,
}

/// 当前选中目标
#[derive(Resource, Default)]
pub struct TargetEntity {
    pub entity_id: Option<u64>,
    pub is_mob: bool,
}

/// 战斗日志 (最近 N 条)
#[derive(Resource, Default)]
pub struct CombatLog {
    pub entries: Vec<String>,
}

impl CombatLog {
    pub fn push(&mut self, msg: String) {
        self.entries.push(msg);
        if self.entries.len() > 10 {
            self.entries.remove(0);
        }
    }
}

/// 游戏配置 (从服务端拉取)
#[derive(Resource, Default)]
pub struct GameConfig {
    pub loaded: bool,
    pub items: Vec<serde_json::Value>,
    pub quests: Vec<serde_json::Value>,
}

/// 输入状态 (移动节流)
#[derive(Resource, Default)]
pub struct InputState {
    pub last_move_time: u64,
    pub last_attack_time: u64,
}

/// 网络连接状态
#[derive(Resource, Default)]
pub struct ConnectionState {
    pub connected: bool,
    pub connecting: bool,
}

/// 面板可见性
#[derive(Resource, Default)]
pub struct PanelVisibility {
    pub inventory: bool,
    pub quest: bool,
    pub combat_log: bool,
}
