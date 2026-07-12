//! ECS 组件定义
//!
//! 组件是 Bevy ECS 中附加到实体上的数据片段。

use bevy::prelude::*;

/// 全局字体资源 (中文字体)
///
/// 在 setup 系统中加载 simhei.ttf，所有创建 Text 的系统都应使用此字体。
#[derive(Resource, Clone)]
pub struct GameFont {
    pub font: Handle<Font>,
}

/// 玩家自己的角色标记
#[derive(Component)]
pub struct Player;

/// 其他玩家标记
#[derive(Component)]
pub struct OtherPlayer {
    pub uid: u64,
    pub name: String,
}

/// 怪物/NPC 标记
#[derive(Component)]
pub struct GameEntity {
    pub entity_id: u64,
    pub def_id: u32,
    pub name: String,
    pub entity_type: EntityType,
}

/// 实体类型
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EntityType {
    Mob,
    Npc,
}

/// 游戏世界坐标 (与 Bevy 的 Transform 分开，因为游戏坐标系 y 向下)
#[derive(Component, Clone, Copy)]
pub struct GamePosition {
    pub x: f32,
    pub y: f32,
}

impl GamePosition {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 目标位置 (用于位置插值，实现丝滑移动)
///
/// 每帧由 `interpolate_position_system` 将 Transform 朝此目标 lerp。
/// `render_system` 只更新此组件，不直接修改 Transform，避免位置跳跃。
#[derive(Component, Clone, Copy)]
pub struct TargetPosition {
    /// 游戏坐标 x (y 向下)
    pub x: f32,
    /// 游戏坐标 y (y 向下)
    pub y: f32,
    /// 是否首次设置 (首次时直接吸附，不插值)
    pub initialized: bool,
}

impl TargetPosition {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            initialized: true,
        }
    }
}

/// HP 条信息 (挂在实体主体上)
#[derive(Component, Clone)]
pub struct HealthBar {
    pub entity_id: u64,
    pub hp: i32,
    pub max_hp: i32,
}

impl HealthBar {
    pub fn new(entity_id: u64, hp: i32, max_hp: i32) -> Self {
        Self { entity_id, hp, max_hp }
    }

    /// HP 百分比 (0.0 ~ 1.0)
    pub fn ratio(&self) -> f32 {
        if self.max_hp <= 0 {
            0.0
        } else {
            (self.hp as f32 / self.max_hp as f32).clamp(0.0, 1.0)
        }
    }
}

/// HP 条前景标记 (子实体，宽度随 HP 变化)
#[derive(Component)]
pub struct HpBarFill {
    pub entity_id: u64,
    pub max_width: f32,
}

/// HP 条背景标记 (父实体，用于查找)
#[derive(Component)]
pub struct HpBarMarker {
    pub entity_id: u64,
}

/// 名称标签标记 (子实体)
#[derive(Component)]
pub struct NameTag {
    pub entity_id: u64,
}

/// 选中目标光环标记
#[derive(Component)]
pub struct SelectionRing {
    pub entity_id: u64,
}

/// 掉落物品标记
#[derive(Component)]
pub struct DroppedItem {
    pub drop_id: u64,
    pub item_id: u32,
    pub count: u32,
}

/// 伤害飘字标记 (自动销毁)
#[derive(Component)]
pub struct DamageText {
    pub timer: Timer,
    pub start_y: f32,
}

/// 伤害事件 (跨系统通信)
#[derive(Event, Clone, Debug)]
pub struct DamageEvent {
    pub target_entity_id: u64,
    pub world_x: f32,
    pub world_y: f32, // 游戏坐标 (y 向下)
    pub damage: i32,
    pub is_crit: bool,
    pub is_miss: bool,
}

/// 获得经验事件
#[derive(Event, Clone, Debug)]
pub struct ExpGainEvent {
    pub amount: u32,
}

/// 玩家死亡事件
#[derive(Event, Clone, Debug)]
pub struct PlayerDeathEvent;

/// 玩家复活事件
#[derive(Event, Clone, Debug)]
pub struct PlayerReviveEvent;
