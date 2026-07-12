// ════════════════════════════════════════════════════════════════
// 游戏状态结构
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use super::utils::*;
use logic_lib::game_proto as gp;
use serde_json::Value;

// ── 公会 (v0.6) ──
#[derive(Debug, Clone)]
pub struct GuildInfo {
    pub name: String,
    pub leader: u64,
    pub members: Vec<u64>,
    pub funds: u32,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub uid: u64,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub dir: u8,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub level: u32,
    pub exp: u32,
    pub atk: i32,
    pub def: i32,
    // 装备
    pub weapon: Option<u32>,
    pub armor: Option<u32>,
    pub accessory: Option<u32>,
    // ── 装备强化等级 (v0.7) ──
    pub weapon_enhance: u32,
    pub armor_enhance: u32,
    pub accessory_enhance: u32,
    // 背包: item_id -> count
    pub inventory: Vec<(u32, u32)>,
    // 技能冷却: skill_id -> last_cast_ms
    pub skill_cooldowns: std::collections::HashMap<u32, u64>,
    // 任务进度: quest_id -> (accepted, progress)
    pub quests: Vec<(u32, u32)>, // (quest_id, kill_count)
    // ── 反外挂追踪 (v0.5) ──
    pub last_move_ms: u64,           // 上次移动时间 (毫秒)
    pub last_attack_ms: u64,         // 上次攻击时间 (毫秒)
    pub last_x: f32,                 // 上次报告的 X 坐标
    pub last_y: f32,                 // 上次报告的 Y 坐标
    pub violation_count: u32,        // 累计违规次数
    // ── 经济系统 (v0.6) ──
    pub current_map: u32,            // 当前地图 ID (1=新手村, 2=森林, 3=沙漠, 4=地下城)
    pub gold: u32,                   // 金币
    // ── 技能树 (v0.6) ──
    pub class: u8,         // 0=未选择, 1=战士, 2=法师, 3=弓手
    pub talent_pts: u32,   // 可用天赋点
    pub talents: Vec<u32>, // 已激活天赋
}

impl PlayerState {
    pub fn new(uid: u64, name: String) -> Self {
        let x = 200.0 + ((uid * 37) % 400) as f32;
        let y = 200.0 + ((uid * 53) % 300) as f32;
        Self {
            uid,
            name,
            x,
            y,
            dir: 2,
            hp: 100,
            max_hp: 100,
            mp: 50,
            max_mp: 50,
            level: 1,
            exp: 0,
            atk: 20,
            def: 5,
            weapon: None,
            armor: None,
            accessory: None,
            weapon_enhance: 0,
            armor_enhance: 0,
            accessory_enhance: 0,
            inventory: vec![(6, 3), (7, 2)], // 初始3个生命药水, 2个法力药水
            skill_cooldowns: std::collections::HashMap::new(),
            quests: Vec::new(),
            last_move_ms: 0,
            last_attack_ms: 0,
            last_x: x,
            last_y: y,
            violation_count: 0,
            current_map: 1,
            gold: 0,
            class: 0,
            talent_pts: 0,
            talents: Vec::new(),
        }
    }

    pub fn total_atk(&self) -> i32 {
        let mut atk = self.atk;
        if let Some(id) = self.weapon {
            if let Some(item) = get_item_def(id) {
                let mult = 1.0 + 0.1 * self.weapon_enhance as f32;
                atk += (item.atk_bonus as f32 * mult) as i32;
            }
        }
        if let Some(id) = self.accessory {
            if let Some(item) = get_item_def(id) {
                let mult = 1.0 + 0.1 * self.accessory_enhance as f32;
                atk += (item.atk_bonus as f32 * mult) as i32;
            }
        }
        atk
    }

    pub fn total_def(&self) -> i32 {
        let mut def = self.def;
        if let Some(id) = self.armor {
            if let Some(item) = get_item_def(id) {
                let mult = 1.0 + 0.1 * self.armor_enhance as f32;
                def += (item.def_bonus as f32 * mult) as i32;
            }
        }
        if let Some(id) = self.accessory {
            if let Some(item) = get_item_def(id) {
                let mult = 1.0 + 0.1 * self.accessory_enhance as f32;
                def += (item.def_bonus as f32 * mult) as i32;
            }
        }
        def
    }

    pub fn to_stats_json(&self) -> String {
        serde_json::json!({
            "uid": self.uid,
            "name": self.name,
            "hp": self.hp,
            "maxHp": self.max_hp,
            "mp": self.mp,
            "maxMp": self.max_mp,
            "level": self.level,
            "exp": self.exp,
            "maxExp": exp_for_level(self.level),
            "x": self.x,
            "y": self.y,
            "atk": self.total_atk(),
            "def": self.total_def(),
            "gold": self.gold,
            "class": self.class,
            "classIcon": CLASS_DEFS.iter().find(|c| c.id == self.class).map(|c| c.icon).unwrap_or(""),
            "talentPts": self.talent_pts,
            "talents": self.talents.clone(),
        })
        .to_string()
    }

    /// 构造 proto 版 PlayerStats（完整字段，用于 5001 下行）
    pub fn to_player_stats(&self) -> gp::PlayerStats {
        gp::PlayerStats {
            uid: self.uid,
            name: self.name.clone(),
            hp: self.hp,
            max_hp: self.max_hp,
            mp: self.mp,
            max_mp: self.max_mp,
            level: self.level,
            exp: self.exp,
            max_exp: exp_for_level(self.level),
            x: self.x,
            y: self.y,
            atk: self.total_atk(),
            def: self.total_def(),
            gold: self.gold,
            class_id: self.class as u32,
            talent_points: self.talent_pts,
            class_icon: CLASS_DEFS
                .iter()
                .find(|c| c.id == self.class)
                .map(|c| c.icon.to_string())
                .unwrap_or_default(),
            talents: self.talents.clone(),
        }
    }

    /// 构造 proto 版 EquipmentUpdate（用于 5004 下行，空槽用 empty=true 表示）
    pub fn to_equipment_proto(&self) -> gp::EquipmentUpdate {
        fn slot(id_opt: Option<u32>, enhance: u32) -> gp::EquipmentSlot {
            match id_opt {
                Some(id) => {
                    let def = get_item_def(id);
                    gp::EquipmentSlot {
                        item_id: id,
                        name: def.map(|d| d.name.to_string()).unwrap_or_default(),
                        icon: def.map(|d| d.icon.to_string()).unwrap_or_default(),
                        enhance_level: enhance,
                        empty: false,
                    }
                }
                None => gp::EquipmentSlot {
                    item_id: 0,
                    name: String::new(),
                    icon: String::new(),
                    enhance_level: 0,
                    empty: true,
                },
            }
        }
        gp::EquipmentUpdate {
            weapon: Some(slot(self.weapon, self.weapon_enhance)),
            armor: Some(slot(self.armor, self.armor_enhance)),
            accessory: Some(slot(self.accessory, self.accessory_enhance)),
        }
    }

    /// 构造 proto 版 QuestUpdate（用于 5005 下行）
    pub fn to_quests_proto(&self) -> gp::QuestUpdate {
        let quests: Vec<gp::QuestEntry> = self
            .quests
            .iter()
            .map(|(qid, progress)| {
                let def = get_quest_def(*qid);
                let target = def.map(|d| d.target_count).unwrap_or(0);
                gp::QuestEntry {
                    quest_id: *qid,
                    name: def.map(|d| d.name.to_string()).unwrap_or_default(),
                    progress: *progress,
                    target,
                    desc: def.map(|d| d.desc.to_string()).unwrap_or_default(),
                    completed: def.map(|d| *progress >= d.target_count).unwrap_or(false),
                }
            })
            .collect();
        gp::QuestUpdate { quests }
    }

    pub fn to_enter_json(&self) -> String {
        serde_json::json!({
            "uid": self.uid,
            "name": self.name,
            "x": self.x,
            "y": self.y,
            "hp": self.hp,
            "maxHp": self.max_hp,
            "level": self.level,
        })
        .to_string()
    }

    pub fn to_list_entry(&self) -> String {
        serde_json::json!({
            "uid": self.uid,
            "name": self.name,
            "x": self.x,
            "y": self.y,
            "hp": self.hp,
            "maxHp": self.max_hp,
            "level": self.level,
        })
        .to_string()
    }

    pub fn to_inventory_json(&self) -> String {
        let items: Vec<Value> = self.inventory.iter().map(|(id, count)| {
            let def = get_item_def(*id);
            serde_json::json!({
                "itemId": id,
                "count": count,
                "name": def.map(|d| d.name).unwrap_or("未知"),
                "type": def.map(|d| d.item_type).unwrap_or("unknown"),
                "icon": def.map(|d| d.icon).unwrap_or("?"),
            })
        }).collect();
        serde_json::json!({ "items": items }).to_string()
    }

    pub fn to_equipment_json(&self) -> String {
        fn item_json(id_opt: Option<u32>, enhance: u32) -> Value {
            match id_opt {
                Some(id) => {
                    let def = get_item_def(id);
                    serde_json::json!({
                        "itemId": id,
                        "name": def.map(|d| d.name).unwrap_or("未知"),
                        "icon": def.map(|d| d.icon).unwrap_or("?"),
                        "enhanceLevel": enhance,
                    })
                }
                None => Value::Null,
            }
        }
        serde_json::json!({
            "weapon": item_json(self.weapon, self.weapon_enhance),
            "armor": item_json(self.armor, self.armor_enhance),
            "accessory": item_json(self.accessory, self.accessory_enhance),
        })
        .to_string()
    }

    pub fn to_quests_json(&self) -> String {
        let quests: Vec<Value> = self.quests.iter().map(|(qid, progress)| {
            let def = get_quest_def(*qid);
            serde_json::json!({
                "questId": qid,
                "name": def.map(|d| d.name).unwrap_or("未知"),
                "desc": def.map(|d| d.desc).unwrap_or(""),
                "progress": progress,
                "target": def.map(|d| d.target_count).unwrap_or(0),
                "completed": def.map(|d| *progress >= d.target_count).unwrap_or(false),
            })
        }).collect();
        serde_json::json!({ "quests": quests }).to_string()
    }

    pub fn to_skills_json(&self) -> String {
        let now = current_millis();
        let skills: Vec<Value> = SKILLS.iter().map(|s| {
            let cd_left = self.skill_cooldowns.get(&s.id)
                .map(|last| {
                    let elapsed = now.saturating_sub(*last);
                    if elapsed < s.cooldown_ms { s.cooldown_ms - elapsed } else { 0 }
                })
                .unwrap_or(0);
            serde_json::json!({
                "skillId": s.id,
                "name": s.name,
                "icon": s.icon,
                "mpCost": s.mp_cost,
                "cooldownMs": s.cooldown_ms,
                "cooldownLeft": cd_left,
                "range": s.range,
            })
        }).collect();
        serde_json::json!({ "skills": skills }).to_string()
    }

    pub fn add_exp(&mut self, exp: u32) -> bool {
        self.exp += exp;
        let need = exp_for_level(self.level);
        if self.exp >= need {
            self.exp -= need;
            self.level += 1;
            self.max_hp += 20;
            self.max_mp += 10;
            self.hp = self.max_hp;
            self.mp = self.max_mp;
            self.talent_pts += 1; // v0.6: 每级获得1天赋点
            self.atk += 5;
            self.def += 2;
            true
        } else {
            false
        }
    }

    pub fn add_item(&mut self, item_id: u32, count: u32) {
        if let Some(entry) = self.inventory.iter_mut().find(|(id, _)| *id == item_id) {
            entry.1 += count;
        } else {
            self.inventory.push((item_id, count));
        }
    }

    pub fn remove_item(&mut self, item_id: u32, count: u32) -> bool {
        if let Some(entry) = self.inventory.iter_mut().find(|(id, _)| *id == item_id) {
            if entry.1 >= count {
                entry.1 -= count;
                if entry.1 == 0 {
                    // Keep zero-count entries or remove? Remove for cleanliness.
                }
                return true;
            }
        }
        false
    }

    pub fn update_quest_progress(&mut self, mob_id: u32) -> bool {
        let mut updated = false;
        for entry in self.quests.iter_mut() {
            let (quest_id, progress) = entry;
            if let Some(def) = get_quest_def(*quest_id) {
                if def.target_mob == mob_id && *progress < def.target_count {
                    *progress += 1;
                    updated = true;
                }
            }
        }
        updated
    }
}

#[derive(Debug, Clone)]
pub struct MobEntity {
    pub entity_id: u64,
    pub def_id: u32,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub spawn_x: f32,
    pub spawn_y: f32,
    pub dir: u8,
    pub hp: i32,
    pub max_hp: i32,
    pub atk: i32,
    pub def: i32,
    pub level: u32,
    #[allow(dead_code)]
    pub exp: u32,
    pub state: MobState,
    pub target_uid: Option<u64>,
    pub last_attack: u64,
    pub last_move: u64,
    pub move_dir: f32, // radians
    pub patrol_tx: Option<f32>,
    pub patrol_ty: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MobState {
    Idle,
    #[allow(dead_code)]
    Patrolling,
    Chasing,
    Attacking,
    Dead,
    #[allow(dead_code)]
    Respawning,
}

impl MobEntity {
    pub fn from_def(entity_id: u64, def_id: u32, x: f32, y: f32) -> Self {
        let def = get_mob_def(def_id).expect("invalid mob def_id");
        MobEntity {
            entity_id,
            def_id,
            name: def.name.to_string(),
            x,
            y,
            spawn_x: x,
            spawn_y: y,
            dir: 2,
            hp: def.max_hp,
            max_hp: def.max_hp,
            atk: def.atk,
            def: def.def,
            level: def.level,
            exp: def.exp,
            state: MobState::Idle,
            target_uid: None,
            last_attack: 0,
            last_move: 0,
            move_dir: 0.0,
            patrol_tx: None,
            patrol_ty: None,
        }
    }

    pub fn to_spawn_json(&self) -> String {
        serde_json::json!({
            "entityId": self.entity_id,
            "defId": self.def_id,
            "name": self.name,
            "x": self.x,
            "y": self.y,
            "hp": self.hp,
            "maxHp": self.max_hp,
            "level": self.level,
            "state": format!("{:?}", self.state),
        })
        .to_string()
    }

    pub fn to_list_entry(&self) -> String {
        self.to_spawn_json()
    }

    /// 转为 proto EntityListEntry (用于 9002 下行)
    pub fn to_entity_list_entry(&self) -> gp::EntityListEntry {
        gp::EntityListEntry {
            entity_id: self.entity_id,
            def_id: self.def_id,
            name: self.name.clone(),
            x: self.x,
            y: self.y,
            hp: self.hp,
            max_hp: self.max_hp,
            level: self.level,
            npc_type: String::new(),
            quest_id: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NpcEntity {
    pub id: u32,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub npc_type: String,
    pub dialog: String,
}

impl NpcEntity {
    pub fn from_def(def: &NpcDef) -> Self {
        NpcEntity {
            id: def.id,
            name: def.name.to_string(),
            x: def.x,
            y: def.y,
            npc_type: def.npc_type.to_string(),
            dialog: def.dialog.to_string(),
        }
    }

    pub fn to_json(&self) -> String {
        let mut json = serde_json::json!({
            "id": self.id,
            "name": self.name,
            "x": self.x,
            "y": self.y,
            "type": self.npc_type,
            "dialog": self.dialog,
        });
        // quest_giver 类型的 NPC 附带可用任务 ID 列表
        if self.npc_type == "quest_giver" {
            let quest_ids: Vec<u32> = QUEST_DEFS.iter().map(|q| q.id).collect();
            json["quests"] = serde_json::json!(quest_ids);
        }
        json.to_string()
    }

    /// 转为 proto EntityListEntry (用于 9002 下行)
    pub fn to_entity_list_entry(&self) -> gp::EntityListEntry {
        let quest_id = if self.npc_type == "quest_giver" {
            QUEST_DEFS.first().map(|q| q.id).unwrap_or(0)
        } else {
            0
        };
        gp::EntityListEntry {
            entity_id: self.id as u64,
            def_id: self.id,
            name: self.name.clone(),
            x: self.x,
            y: self.y,
            hp: 0,
            max_hp: 0,
            level: 0,
            npc_type: self.npc_type.clone(),
            quest_id,
        }
    }
}

/// 掉落物
#[derive(Debug, Clone)]
pub struct ItemDrop {
    pub drop_id: u64,
    pub item_id: u32,
    pub x: f32,
    pub y: f32,
    pub count: u32,
}

impl ItemDrop {
    pub fn to_json(&self) -> String {
        let def = get_item_def(self.item_id);
        serde_json::json!({
            "dropId": self.drop_id,
            "itemId": self.item_id,
            "count": self.count,
            "x": self.x,
            "y": self.y,
            "name": def.map(|d| d.name).unwrap_or("未知"),
            "icon": def.map(|d| d.icon).unwrap_or("?"),
        })
        .to_string()
    }
}
