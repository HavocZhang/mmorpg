//! ECS 组件定义
//!
//! 组件是 Bevy ECS 中附加到实体上的数据片段。

use bevy::prelude::*;

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

    /// 转换为 Bevy 世界坐标 (y 轴翻转)
    pub fn to_bevy(&self) -> Vec3 {
        Vec3::new(self.x, -self.y, 0.0)
    }
}

/// HP 条信息
#[derive(Component, Clone)]
pub struct HealthBar {
    pub hp: i32,
    pub max_hp: i32,
}

impl HealthBar {
    pub fn new(hp: i32, max_hp: i32) -> Self {
        Self { hp, max_hp }
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

/// 实体名称标签 (用于查找名称文本实体)
#[derive(Component)]
pub struct EntityLabel {
    pub entity_id: u64,
}

/// HP 条容器标记 (用于查找 HP 条实体)
#[derive(Component)]
pub struct HpBarMarker {
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
