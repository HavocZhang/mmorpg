// ════════════════════════════════════════════════════════════════
// config_loader.rs — 配置数据加载器
//
// 从 JSON 文件加载配置数据；文件不存在或解析失败时回退到 constants.rs
// 里的 const 默认值，保证服务端在缺失 config/ 目录时仍能正常启动。
//
// config/ 目录的 JSON 是新的 single source of truth；Rust const 仅作
// fallback 保留，get_item_def/get_mob_def/get_quest_def/get_skill_def
// 等查询函数继续直接读 const，未受影响。
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── 配置结构体（JSON <-> Rust） ──

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SkillConfig {
    pub id: u32,
    pub name: String,
    pub dmg_multiplier: f32,
    pub mp_cost: i32,
    pub cooldown_ms: u64,
    pub range: f32,
    pub icon: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MobConfig {
    pub id: u32,
    pub name: String,
    pub max_hp: i32,
    pub atk: i32,
    pub def: i32,
    pub exp: u32,
    pub level: u32,
    pub radius: f32,
    pub detect_range: f32,
    pub attack_range: f32,
    pub attack_cd_ms: u64,
    pub move_speed: f32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ItemConfig {
    pub id: u32,
    pub name: String,
    pub item_type: String,
    pub value: u32,
    pub icon: String,
    pub hp_restore: i32,
    pub mp_restore: i32,
    pub atk_bonus: i32,
    pub def_bonus: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct QuestConfig {
    pub id: u32,
    pub name: String,
    pub desc: String,
    pub target_mob: u32,
    pub target_count: u32,
    pub exp_reward: u32,
    pub item_reward: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClassConfig {
    pub id: u8,
    pub name: String,
    pub icon: String,
    pub atk_bonus: i32,
    pub def_bonus: i32,
    pub hp_bonus: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TalentConfig {
    pub id: u32,
    pub name: String,
    pub class: u8,
    pub atk: i32,
    pub def: i32,
    pub hp: i32,
    pub icon: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NpcConfig {
    pub id: u32,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub npc_type: String,
    pub dialog: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MapConfig {
    pub id: u32,
    pub name: String,
    pub bounds: [f32; 4], // [min_x, min_y, max_x, max_y]
    pub bg_color: String,
    pub mob_types: Vec<u32>,
    pub portal_npc_ids: Vec<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShopItemConfig {
    pub item_id: u32,
    pub price: u32,
    pub sell_price: u32,
    pub stock: Option<u32>, // None = 无限库存
}

// ── const -> Config 转换（fallback 用） ──

impl From<&SkillDef> for SkillConfig {
    fn from(s: &SkillDef) -> Self {
        SkillConfig {
            id: s.id,
            name: s.name.to_string(),
            dmg_multiplier: s.dmg_multiplier,
            mp_cost: s.mp_cost,
            cooldown_ms: s.cooldown_ms,
            range: s.range,
            icon: s.icon.to_string(),
        }
    }
}

impl From<&MobDef> for MobConfig {
    fn from(m: &MobDef) -> Self {
        MobConfig {
            id: m.id,
            name: m.name.to_string(),
            max_hp: m.max_hp,
            atk: m.atk,
            def: m.def,
            exp: m.exp,
            level: m.level,
            radius: m.radius,
            detect_range: m.detect_range,
            attack_range: m.attack_range,
            attack_cd_ms: m.attack_cd_ms,
            move_speed: m.move_speed,
        }
    }
}

impl From<&ItemDef> for ItemConfig {
    fn from(i: &ItemDef) -> Self {
        ItemConfig {
            id: i.id,
            name: i.name.to_string(),
            item_type: i.item_type.to_string(),
            value: i.value,
            icon: i.icon.to_string(),
            hp_restore: i.hp_restore,
            mp_restore: i.mp_restore,
            atk_bonus: i.atk_bonus,
            def_bonus: i.def_bonus,
        }
    }
}

impl From<&QuestDef> for QuestConfig {
    fn from(q: &QuestDef) -> Self {
        QuestConfig {
            id: q.id,
            name: q.name.to_string(),
            desc: q.desc.to_string(),
            target_mob: q.target_mob,
            target_count: q.target_count,
            exp_reward: q.exp_reward,
            item_reward: q.item_reward,
        }
    }
}

impl From<&ClassDef> for ClassConfig {
    fn from(c: &ClassDef) -> Self {
        ClassConfig {
            id: c.id,
            name: c.name.to_string(),
            icon: c.icon.to_string(),
            atk_bonus: c.atk_bonus,
            def_bonus: c.def_bonus,
            hp_bonus: c.hp_bonus,
        }
    }
}

impl From<&TalentDef> for TalentConfig {
    fn from(t: &TalentDef) -> Self {
        TalentConfig {
            id: t.id,
            name: t.name.to_string(),
            class: t.class,
            atk: t.atk,
            def: t.def,
            hp: t.hp,
            icon: t.icon.to_string(),
        }
    }
}

impl From<&NpcDef> for NpcConfig {
    fn from(n: &NpcDef) -> Self {
        NpcConfig {
            id: n.id,
            name: n.name.to_string(),
            x: n.x,
            y: n.y,
            npc_type: n.npc_type.to_string(),
            dialog: n.dialog.to_string(),
        }
    }
}

impl From<&MapDef> for MapConfig {
    fn from(m: &MapDef) -> Self {
        MapConfig {
            id: m.id,
            name: m.name.to_string(),
            bounds: [m.bounds.0, m.bounds.1, m.bounds.2, m.bounds.3],
            bg_color: m.bg_color.to_string(),
            mob_types: m.mob_types.to_vec(),
            portal_npc_ids: m.portal_npc_ids.to_vec(),
        }
    }
}

impl From<&ShopItem> for ShopItemConfig {
    fn from(s: &ShopItem) -> Self {
        ShopItemConfig {
            item_id: s.item_id,
            price: s.price,
            sell_price: s.sell_price,
            stock: s.stock,
        }
    }
}

// ── 配置集合 ──

#[derive(Debug, Clone, Default)]
pub struct GameConfig {
    pub skills: Vec<SkillConfig>,
    pub mobs: Vec<MobConfig>,
    pub items: Vec<ItemConfig>,
    pub quests: Vec<QuestConfig>,
    pub classes: Vec<ClassConfig>,
    pub talents: Vec<TalentConfig>,
    pub npcs: Vec<NpcConfig>,
    pub maps: Vec<MapConfig>,
    pub shop_items: Vec<ShopItemConfig>,
}

impl GameConfig {
    /// 从 config/ 目录加载 JSON；文件不存在或解析失败则用 const 默认值。
    ///
    /// 搜索顺序：先 `config/`（项目根运行），再 `../config/`（从 logic-lib
    /// 子目录运行）。两处都不存在则全部回退到 const。
    pub fn load() -> Self {
        let dir = resolve_config_dir();
        let mut cfg = Self::default();

        cfg.skills = load_or_fallback(&format!("{}/skills.json", dir), || {
            SKILLS.iter().map(Into::into).collect()
        });
        cfg.mobs = load_or_fallback(&format!("{}/mobs.json", dir), || {
            MOB_DEFS.iter().map(Into::into).collect()
        });
        cfg.items = load_or_fallback(&format!("{}/items.json", dir), || {
            ITEM_DEFS.iter().map(Into::into).collect()
        });
        cfg.quests = load_or_fallback(&format!("{}/quests.json", dir), || {
            QUEST_DEFS.iter().map(Into::into).collect()
        });
        cfg.classes = load_or_fallback(&format!("{}/classes.json", dir), || {
            CLASS_DEFS.iter().map(Into::into).collect()
        });
        cfg.talents = load_or_fallback(&format!("{}/talents.json", dir), || {
            TALENTS.iter().map(Into::into).collect()
        });
        cfg.npcs = load_or_fallback(&format!("{}/npcs.json", dir), || {
            NPC_DEFS.iter().map(Into::into).collect()
        });
        cfg.maps = load_or_fallback(&format!("{}/maps.json", dir), || {
            MAP_DEFS.iter().map(Into::into).collect()
        });
        cfg.shop_items = load_or_fallback(&format!("{}/shop_items.json", dir), || {
            SHOP_ITEMS.iter().map(Into::into).collect()
        });

        tracing::info!(
            "配置加载: {} 技能, {} 怪物, {} 物品, {} 任务, {} 职业, {} 天赋, {} NPC, {} 地图, {} 商品 (dir={})",
            cfg.skills.len(),
            cfg.mobs.len(),
            cfg.items.len(),
            cfg.quests.len(),
            cfg.classes.len(),
            cfg.talents.len(),
            cfg.npcs.len(),
            cfg.maps.len(),
            cfg.shop_items.len(),
            dir,
        );

        cfg
    }

    /// 序列化为 JSON 字符串（供客户端通过 9100 消息拉取）。
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "skills": self.skills,
            "mobs": self.mobs,
            "items": self.items,
            "quests": self.quests,
            "classes": self.classes,
            "talents": self.talents,
            "npcs": self.npcs,
            "maps": self.maps,
            "shopItems": self.shop_items,
        })
        .to_string()
    }
}

/// 解析 config 目录：优先 `config/`，其次 `../config/`；都找不到返回 `config`
/// （后续每个文件读取失败时会走 const fallback，不影响启动）。
fn resolve_config_dir() -> String {
    for candidate in ["config", "../config"] {
        if Path::new(&format!("{}/skills.json", candidate)).exists() {
            return candidate.to_string();
        }
    }
    "config".to_string()
}

/// 读取 JSON 文件并反序列化；任何错误都调用 `fallback` 生成默认值。
fn load_or_fallback<T, F>(path: &str, fallback: F) -> Vec<T>
where
    T: for<'de> Deserialize<'de>,
    F: FnOnce() -> Vec<T>,
{
    match std::fs::read_to_string(path) {
        Ok(data) => match serde_json::from_str::<Vec<T>>(&data) {
            Ok(v) if !v.is_empty() => v,
            Ok(_) => {
                tracing::warn!(path, "配置文件为空, 使用 const 默认值");
                fallback()
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "配置文件解析失败, 使用 const 默认值");
                fallback()
            }
        },
        Err(_) => fallback(),
    }
}

// ── 全局配置（OnceLock） ──

static GAME_CONFIG: std::sync::OnceLock<GameConfig> = std::sync::OnceLock::new();

/// 获取全局配置单例（首次调用时加载，之后直接返回引用）。
pub fn get_config() -> &'static GameConfig {
    GAME_CONFIG.get_or_init(GameConfig::load)
}
