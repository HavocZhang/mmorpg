//! Mock 游戏逻辑服 (完整 MMORPG 版)
//!
//! 模拟真实游戏服务器的行为：
//! 1. 维护在线玩家状态（位置、HP、MP、等级、经验、背包、装备、技能、任务）
//! 2. NPC/怪物实体（巡逻、仇恨、攻击、死亡、复活、掉落）
//! 3. 物品系统（拾取、使用、装备、卸下）
//! 4. 任务系统（接取、进度、完成、奖励）
//! 5. 处理移动同步、聊天、战斗、附近玩家/NPC查询
//! 6. 玩家上线/下线时广播通知
//!
//! 运行方式: cargo run --bin logic-server
//!
//! 消息协议约定:
//!   上行 (Client -> Gateway -> LogicServer):
//!     100:  初始化/请求玩家列表
//!     1001: 基础攻击        {"targetUid":12345}
//!     1002: 技能攻击        {"skillId":1,"targetUid":12345}
//!     1003: 拾取物品        {"itemId":1}
//!     1004: 装备/卸下       {"itemId":1,"slot":"weapon"}
//!     1005: 接受任务        {"questId":1}
//!     1006: 完成任务        {"questId":1}
//!     1007: NPC交互         {"npcId":1}
//!     1008: 使用物品        {"itemId":1}
//!     2001: 聊天            {"text":"hello","channel":"world"}
//!     3001: 移动            {"x":100.0,"y":200.0,"dir":0}
//!     4001: 查询附近玩家
//!     4002: 查询附近实体(NPC/怪物)
//!
//!   下行 (LogicServer -> Gateway -> Client):
//!     5001: 玩家属性        {"uid","name","hp","maxHp","mp","maxMp","level","exp","maxExp","x","y","atk","def"}
//!     5002: 经验更新        {"exp","maxExp","level"}
//!     5003: 背包更新        {"items":[...]}
//!     5004: 装备更新        {"weapon","armor","accessory"}
//!     5005: 任务更新        {"quests":[...]}
//!     5006: NPC对话         {"npcId","name","dialog","options":[...]}
//!     6001: 战斗结果        {"targetUid","dmg","targetHp","crit"}
//!     6002: 实体状态        {"entityId","hp","maxHp","state","x","y"}
//!     6003: 实体死亡/掉落   {"entityId","killer","drops":[...]}
//!     7001: 聊天ACK
//!     7002: 聊天广播
//!     8001: 玩家位置更新
//!     8002: 玩家进入
//!     8003: 玩家离开
//!     8004: 实体位置更新    {"entityId","x","y","dir"}
//!     9001: 玩家列表
//!     9002: 实体列表        {"npcs":[...],"mobs":[...]}

use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use serde_json::Value;
use tonic::{transport::Server, Request, Response, Status};

use rust_mmo_gate::grpc_router::proto::gate::{
    logic_service_server::{LogicService, LogicServiceServer},
    DownstreamMessage, ForwardBatchRequest, ForwardRequest, ForwardResponse,
    PlayerOfflineRequest, PlayerOfflineResponse, PlayerOnlineRequest, PlayerOnlineResponse,
};

// ════════════════════════════════════════════════════════════════
// 常量定义
// ════════════════════════════════════════════════════════════════

/// 世界尺寸
const WORLD_W: f32 = 1600.0;
const WORLD_H: f32 = 1200.0;

/// 技能定义
struct SkillDef {
    id: u32,
    name: &'static str,
    dmg_multiplier: f32,
    mp_cost: i32,
    cooldown_ms: u64,
    range: f32,
    icon: &'static str,
}

const SKILLS: &[SkillDef] = &[
    SkillDef { id: 1, name: "普通攻击", dmg_multiplier: 1.0,  mp_cost: 0,  cooldown_ms: 800,  range: 80.0,  icon: "⚔" },
    SkillDef { id: 2, name: "重击",     dmg_multiplier: 2.0,  mp_cost: 10, cooldown_ms: 2000, range: 80.0,  icon: "💥" },
    SkillDef { id: 3, name: "火球术",   dmg_multiplier: 3.0,  mp_cost: 20, cooldown_ms: 3000, range: 200.0, icon: "🔥" },
    SkillDef { id: 4, name: "冰冻",     dmg_multiplier: 1.5,  mp_cost: 15, cooldown_ms: 4000, range: 150.0, icon: "❄" },
    SkillDef { id: 5, name: "治疗术",   dmg_multiplier: 0.0,  mp_cost: 25, cooldown_ms: 5000, range: 0.0,   icon: "💚" },
];

/// 怪物定义
struct MobDef {
    id: u32,
    name: &'static str,
    max_hp: i32,
    atk: i32,
    def: i32,
    exp: u32,
    level: u32,
    radius: f32,       // 巡逻半径
    detect_range: f32, // 仇恨范围
    attack_range: f32,
    attack_cd_ms: u64,
    move_speed: f32,
}

const MOB_DEFS: &[MobDef] = &[
    MobDef { id: 1, name: "史莱姆",     max_hp: 50,  atk: 8,  def: 2,  exp: 20,  level: 1, radius: 80.0,  detect_range: 120.0, attack_range: 30.0, attack_cd_ms: 2000, move_speed: 0.8 },
    MobDef { id: 2, name: "哥布林",     max_hp: 80,  atk: 12, def: 4,  exp: 35,  level: 2, radius: 100.0, detect_range: 150.0, attack_range: 35.0, attack_cd_ms: 1800, move_speed: 1.2 },
    MobDef { id: 3, name: "骷髅战士",   max_hp: 120, atk: 18, def: 8,  exp: 60,  level: 4, radius: 90.0,  detect_range: 140.0, attack_range: 35.0, attack_cd_ms: 1500, move_speed: 1.0 },
    MobDef { id: 4, name: "暗影法师",   max_hp: 90,  atk: 25, def: 3,  exp: 80,  level: 5, radius: 120.0, detect_range: 200.0, attack_range: 180.0, attack_cd_ms: 2200, move_speed: 0.6 },
    MobDef { id: 5, name: "岩石巨人",   max_hp: 300, atk: 30, def: 20, exp: 200, level: 8, radius: 60.0,  detect_range: 100.0, attack_range: 40.0, attack_cd_ms: 2500, move_speed: 0.5 },
];

/// NPC 定义
struct NpcDef {
    id: u32,
    name: &'static str,
    x: f32,
    y: f32,
    npc_type: &'static str, // "merchant", "quest_giver", "healer"
    dialog: &'static str,
}

const NPC_DEFS: &[NpcDef] = &[
    NpcDef { id: 1, name: "村长·李四",   x: 200.0, y: 200.0, npc_type: "quest_giver", dialog: "欢迎来到新手村！最近附近出现了不少怪物，能帮我们清理一些吗？" },
    NpcDef { id: 2, name: "商人·王五",   x: 1400.0, y: 200.0, npc_type: "merchant", dialog: "各种药水、装备应有尽有，来看看吧！" },
    NpcDef { id: 3, name: "治疗师·赵六", x: 800.0, y: 600.0, npc_type: "healer", dialog: "需要治疗吗？我可以免费为你恢复全部生命和法力。" },
    NpcDef { id: 4, name: "铁匠·孙七",   x: 1200.0, y: 800.0, npc_type: "merchant", dialog: "好剑配英雄！我可以帮你强化装备。" },
    NpcDef { id: 5, name: "公会会长",    x: 400.0, y: 1000.0, npc_type: "quest_giver", dialog: "想加入冒险者公会吗？先证明你的实力！" },
];

/// 物品定义
#[derive(Clone)]
struct ItemDef {
    id: u32,
    name: &'static str,
    item_type: &'static str, // "weapon", "armor", "accessory", "potion", "material"
    #[allow(dead_code)]
    value: u32,
    icon: &'static str,
    hp_restore: i32,
    mp_restore: i32,
    atk_bonus: i32,
    def_bonus: i32,
}

const ITEM_DEFS: &[ItemDef] = &[
    ItemDef { id: 1, name: "铁剑",       item_type: "weapon",    value: 100,  icon: "🗡", hp_restore: 0,  mp_restore: 0,  atk_bonus: 15, def_bonus: 0  },
    ItemDef { id: 2, name: "钢剑",       item_type: "weapon",    value: 300,  icon: "⚔",  hp_restore: 0,  mp_restore: 0,  atk_bonus: 30, def_bonus: 0  },
    ItemDef { id: 3, name: "皮甲",       item_type: "armor",     value: 150,  icon: "🛡", hp_restore: 0,  mp_restore: 0,  atk_bonus: 0,  def_bonus: 10 },
    ItemDef { id: 4, name: "铁甲",       item_type: "armor",     value: 400,  icon: "🛡", hp_restore: 0,  mp_restore: 0,  atk_bonus: 0,  def_bonus: 25 },
    ItemDef { id: 5, name: "力量戒指",   item_type: "accessory", value: 200,  icon: "💍", hp_restore: 0,  mp_restore: 0,  atk_bonus: 10, def_bonus: 5  },
    ItemDef { id: 6, name: "生命药水",   item_type: "potion",    value: 50,   icon: "🧪", hp_restore: 50, mp_restore: 0,  atk_bonus: 0,  def_bonus: 0  },
    ItemDef { id: 7, name: "法力药水",   item_type: "potion",    value: 50,   icon: "🔵", hp_restore: 0,  mp_restore: 30, atk_bonus: 0,  def_bonus: 0  },
    ItemDef { id: 8, name: "全恢复药水", item_type: "potion",    value: 150,  icon: "💎", hp_restore: 100,mp_restore: 50, atk_bonus: 0,  def_bonus: 0  },
    ItemDef { id: 9, name: "史莱姆凝胶", item_type: "material",  value: 10,   icon: "🟢", hp_restore: 0,  mp_restore: 0,  atk_bonus: 0,  def_bonus: 0  },
    ItemDef { id: 10,name: "哥布林耳朵", item_type: "material",  value: 15,   icon: "👂", hp_restore: 0,  mp_restore: 0,  atk_bonus: 0,  def_bonus: 0  },
];

/// 任务定义
struct QuestDef {
    id: u32,
    name: &'static str,
    desc: &'static str,
    target_mob: u32,
    target_count: u32,
    exp_reward: u32,
    item_reward: u32, // item id
}

const QUEST_DEFS: &[QuestDef] = &[
    QuestDef { id: 1, name: "清除史莱姆",   desc: "消灭5只史莱姆",            target_mob: 1, target_count: 5,  exp_reward: 100,  item_reward: 6 },
    QuestDef { id: 2, name: "哥布林威胁",   desc: "消灭3只哥布林",            target_mob: 2, target_count: 3,  exp_reward: 200,  item_reward: 7 },
    QuestDef { id: 3, name: "骷髅清剿",     desc: "消灭2只骷髅战士",          target_mob: 3, target_count: 2,  exp_reward: 350,  item_reward: 1 },
    QuestDef { id: 4, name: "暗影威胁",     desc: "消灭1只暗影法师",          target_mob: 4, target_count: 1,  exp_reward: 500,  item_reward: 2 },
    QuestDef { id: 5, name: "巨人杀手",     desc: "消灭1只岩石巨人",          target_mob: 5, target_count: 1,  exp_reward: 1000, item_reward: 4 },
];

/// 升级所需经验
fn exp_for_level(level: u32) -> u32 {
    100 * level * level
}

// ════════════════════════════════════════════════════════════════
// 游戏状态结构
// ════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct PlayerState {
    uid: u64,
    name: String,
    x: f32,
    y: f32,
    dir: u8,
    hp: i32,
    max_hp: i32,
    mp: i32,
    max_mp: i32,
    level: u32,
    exp: u32,
    atk: i32,
    def: i32,
    // 装备
    weapon: Option<u32>,
    armor: Option<u32>,
    accessory: Option<u32>,
    // 背包: item_id -> count
    inventory: Vec<(u32, u32)>,
    // 技能冷却: skill_id -> last_cast_ms
    skill_cooldowns: std::collections::HashMap<u32, u64>,
    // 任务进度: quest_id -> (accepted, progress)
    quests: Vec<(u32, u32)>, // (quest_id, kill_count)
    // ── 反外挂追踪 (v0.5) ──
    last_move_ms: u64,           // 上次移动时间 (毫秒)
    last_attack_ms: u64,         // 上次攻击时间 (毫秒)
    last_x: f32,                 // 上次报告的 X 坐标
    last_y: f32,                 // 上次报告的 Y 坐标
    violation_count: u32,        // 累计违规次数
}

impl PlayerState {
    fn new(uid: u64, name: String) -> Self {
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
            inventory: vec![(6, 3), (7, 2)], // 初始3个生命药水, 2个法力药水
            skill_cooldowns: std::collections::HashMap::new(),
            quests: Vec::new(),
            last_move_ms: 0,
            last_attack_ms: 0,
            last_x: x,
            last_y: y,
            violation_count: 0,
        }
    }

    fn total_atk(&self) -> i32 {
        let mut atk = self.atk;
        if let Some(id) = self.weapon {
            if let Some(item) = get_item_def(id) {
                atk += item.atk_bonus;
            }
        }
        if let Some(id) = self.accessory {
            if let Some(item) = get_item_def(id) {
                atk += item.atk_bonus;
            }
        }
        atk
    }

    fn total_def(&self) -> i32 {
        let mut def = self.def;
        if let Some(id) = self.armor {
            if let Some(item) = get_item_def(id) {
                def += item.def_bonus;
            }
        }
        if let Some(id) = self.accessory {
            if let Some(item) = get_item_def(id) {
                def += item.def_bonus;
            }
        }
        def
    }

    fn to_stats_json(&self) -> String {
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
        })
        .to_string()
    }

    fn to_enter_json(&self) -> String {
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

    fn to_list_entry(&self) -> String {
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

    fn to_inventory_json(&self) -> String {
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

    fn to_equipment_json(&self) -> String {
        fn item_json(id_opt: Option<u32>) -> Value {
            match id_opt {
                Some(id) => {
                    let def = get_item_def(id);
                    serde_json::json!({
                        "itemId": id,
                        "name": def.map(|d| d.name).unwrap_or("未知"),
                        "icon": def.map(|d| d.icon).unwrap_or("?"),
                    })
                }
                None => Value::Null,
            }
        }
        serde_json::json!({
            "weapon": item_json(self.weapon),
            "armor": item_json(self.armor),
            "accessory": item_json(self.accessory),
        })
        .to_string()
    }

    fn to_quests_json(&self) -> String {
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

    fn to_skills_json(&self) -> String {
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

    fn add_exp(&mut self, exp: u32) -> bool {
        self.exp += exp;
        let need = exp_for_level(self.level);
        if self.exp >= need {
            self.exp -= need;
            self.level += 1;
            self.max_hp += 20;
            self.max_mp += 10;
            self.hp = self.max_hp;
            self.mp = self.max_mp;
            self.atk += 5;
            self.def += 2;
            true
        } else {
            false
        }
    }

    fn add_item(&mut self, item_id: u32, count: u32) {
        if let Some(entry) = self.inventory.iter_mut().find(|(id, _)| *id == item_id) {
            entry.1 += count;
        } else {
            self.inventory.push((item_id, count));
        }
    }

    fn remove_item(&mut self, item_id: u32, count: u32) -> bool {
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

    fn update_quest_progress(&mut self, mob_id: u32) -> bool {
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
struct MobEntity {
    entity_id: u64,
    def_id: u32,
    name: String,
    x: f32,
    y: f32,
    spawn_x: f32,
    spawn_y: f32,
    dir: u8,
    hp: i32,
    max_hp: i32,
    atk: i32,
    def: i32,
    level: u32,
    exp: u32,
    state: MobState,
    target_uid: Option<u64>,
    last_attack: u64,
    last_move: u64,
    move_dir: f32, // radians
    patrol_tx: Option<f32>,
    patrol_ty: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
enum MobState {
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
    fn from_def(entity_id: u64, def_id: u32, x: f32, y: f32) -> Self {
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

    fn to_spawn_json(&self) -> String {
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

    fn to_list_entry(&self) -> String {
        self.to_spawn_json()
    }
}

#[derive(Debug, Clone)]
struct NpcEntity {
    id: u32,
    name: String,
    x: f32,
    y: f32,
    npc_type: String,
    dialog: String,
}

impl NpcEntity {
    fn from_def(def: &NpcDef) -> Self {
        NpcEntity {
            id: def.id,
            name: def.name.to_string(),
            x: def.x,
            y: def.y,
            npc_type: def.npc_type.to_string(),
            dialog: def.dialog.to_string(),
        }
    }

    fn to_json(&self) -> String {
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
}

/// 掉落物
#[derive(Debug, Clone)]
struct ItemDrop {
    drop_id: u64,
    item_id: u32,
    x: f32,
    y: f32,
    count: u32,
}

impl ItemDrop {
    fn to_json(&self) -> String {
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

// ════════════════════════════════════════════════════════════════
// 辅助函数
// ════════════════════════════════════════════════════════════════

fn get_item_def(id: u32) -> Option<&'static ItemDef> {
    ITEM_DEFS.iter().find(|d| d.id == id)
}

fn get_mob_def(id: u32) -> Option<&'static MobDef> {
    MOB_DEFS.iter().find(|d| d.id == id)
}

fn get_quest_def(id: u32) -> Option<&'static QuestDef> {
    QUEST_DEFS.iter().find(|d| d.id == id)
}

fn get_skill_def(id: u32) -> Option<&'static SkillDef> {
    SKILLS.iter().find(|d| d.id == id)
}

fn current_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn distance(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    (dx * dx + dy * dy).sqrt()
}

fn dm(target_uid: u64, msg_id: u32, payload: String, priority: u32) -> DownstreamMessage {
    DownstreamMessage {
        target_uid,
        msg_id,
        payload: payload.into_bytes(),
        priority,
    }
}

// ════════════════════════════════════════════════════════════════
// 逻辑服实现
// ════════════════════════════════════════════════════════════════

pub struct GameState {
    pub players: DashMap<u64, PlayerState>,
    pub mobs: DashMap<u64, MobEntity>,
    pub npcs: Vec<NpcEntity>,
    pub drops: DashMap<u64, ItemDrop>,
    pub next_entity_id: std::sync::atomic::AtomicU64,
    pub next_drop_id: std::sync::atomic::AtomicU64,
    pub online_count: std::sync::atomic::AtomicU64,
    pub last_mob_tick: std::sync::atomic::AtomicU64,
    pub party_mgr: logic_lib::party::PartyManager,
}

pub struct MockLogicService {
    state: Arc<GameState>,
    db: Option<Arc<logic_lib::db::Database>>,
}

impl Default for MockLogicService {
    fn default() -> Self {
        let state = GameState {
            players: DashMap::new(),
            mobs: DashMap::new(),
            npcs: NPC_DEFS.iter().map(NpcEntity::from_def).collect(),
            drops: DashMap::new(),
            next_entity_id: std::sync::atomic::AtomicU64::new(10000),
            next_drop_id: std::sync::atomic::AtomicU64::new(20000),
            online_count: std::sync::atomic::AtomicU64::new(0),
            last_mob_tick: std::sync::atomic::AtomicU64::new(0),
            party_mgr: logic_lib::party::PartyManager::new(),
        };

        // Spawn 初始怪物
        let mob_spawns: &[(u32, f32, f32)] = &[
            (1, 500.0, 400.0), (1, 600.0, 350.0), (1, 700.0, 450.0),
            (1, 450.0, 500.0), (1, 800.0, 380.0), (1, 550.0, 550.0),
            (2, 1000.0, 500.0), (2, 1100.0, 450.0), (2, 900.0, 550.0), (2, 1200.0, 400.0),
            (3, 1300.0, 700.0), (3, 1400.0, 650.0), (3, 1200.0, 750.0),
            (4, 700.0, 900.0), (4, 900.0, 1000.0),
            (5, 1100.0, 1000.0),
        ];
        for (def_id, x, y) in mob_spawns {
            let eid = state.next_entity_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            state.mobs.insert(eid, MobEntity::from_def(eid, *def_id, *x, *y));
        }

        println!("[LogicServer] 已生成 {} 个NPC, {} 个怪物", state.npcs.len(), state.mobs.len());

        let state = Arc::new(state);

        // ====== 后台游戏循环：独立驱动怪物 AI ======
        let bg = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                bg.tick_mob_ai(0);
            }
        });

        MockLogicService { state, db: None }
    }
}

#[tonic::async_trait]
impl LogicService for MockLogicService {
    async fn forward_message(
        &self,
        request: Request<ForwardRequest>,
    ) -> Result<Response<ForwardResponse>, Status> {
        let req = request.into_inner();
        let response = self.state.process_message(req.player_uid, req.msg_id, &req.payload);
        Ok(Response::new(response))
    }

    async fn forward_message_batch(
        &self,
        request: Request<ForwardBatchRequest>,
    ) -> Result<Response<ForwardResponse>, Status> {
        let req = request.into_inner();
        let mut all_downstream = Vec::new();
        for msg in req.messages {
            let resp = self.state.process_message(msg.player_uid, msg.msg_id, &msg.payload);
            all_downstream.extend(resp.messages);
        }
        Ok(Response::new(ForwardResponse {
            messages: all_downstream,
        }))
    }

    async fn player_online(
        &self,
        request: Request<PlayerOnlineRequest>,
    ) -> Result<Response<PlayerOnlineResponse>, Status> {
        let req = request.into_inner();
        let uid = req.player_uid;
        let count = self.state.online_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;

        // 尝试从 DB 加载已有角色
        let (name, mut player) = if let Some(ref db) = self.db {
            if let Ok(Some(data)) = db.load_player(uid).await {
                let name = data["name"].as_str().unwrap_or("Player").to_string();
                let mut p = PlayerState::new(uid, name.clone());
                p.level = data["level"].as_i64().unwrap_or(1) as u32;
                p.exp = data["exp"].as_u64().unwrap_or(0) as u32;
                p.hp = data["hp"].as_i64().unwrap_or(100) as i32;
                p.max_hp = data["maxHp"].as_i64().unwrap_or(100) as i32;
                p.mp = data["mp"].as_i64().unwrap_or(50) as i32;
                p.max_mp = data["maxMp"].as_i64().unwrap_or(50) as i32;
                p.atk = data["atk"].as_i64().unwrap_or(20) as i32;
                p.def = data["def"].as_i64().unwrap_or(5) as i32;
                p.weapon = data["weapon"].as_u64().map(|v| v as u32).or(data["weapon"].as_i64().map(|v| v as u32));
                p.armor = data["armor"].as_u64().map(|v| v as u32).or(data["armor"].as_i64().map(|v| v as u32));
                p.accessory = data["accessory"].as_u64().map(|v| v as u32).or(data["accessory"].as_i64().map(|v| v as u32));
                if let Some(items) = data["inventory"].as_array() {
                    p.inventory = items.iter().map(|i| (i["itemId"].as_u64().unwrap_or(0) as u32, i["count"].as_u64().unwrap_or(1) as u32)).collect();
                }
                if let Some(qs) = data["quests"].as_array() {
                    p.quests = qs.iter().map(|q| (q["questId"].as_u64().unwrap_or(0) as u32, q["progress"].as_u64().unwrap_or(0) as u32)).collect();
                }
                println!("[LogicServer] 从DB加载角色: uid={} name={} lv={}", uid, name, p.level);
                (name, p)
            } else {
                let name = format!("Player{}", uid);
                let p = PlayerState::new(uid, name.clone());
                (name, p)
            }
        } else {
            let name = format!("Player{}", uid);
            let p = PlayerState::new(uid, name.clone());
            (name, p)
        };

        println!(
            "[LogicServer] 玩家上线: uid={} session={} gate={} (在线: {})",
            uid, req.session_id, req.gate_node, count
        );

        let mut messages = Vec::new();

        // 1. 给自己发玩家属性 (5001)
        messages.push(dm(uid, 5001, player.to_stats_json(), 1));

        // 2. 广播玩家进入通知 (8002)
        messages.push(dm(0, 8002, player.to_enter_json(), 1));

        // 3. 给自己发所有已在线玩家的进入通知 (8002)
        for entry in self.state.players.iter() {
            if entry.uid != uid {
                messages.push(dm(uid, 8002, entry.to_enter_json(), 1));
            }
        }

        // 4. 给自己发玩家列表 (9001)
        let list: Vec<String> = self.state.players
            .iter()
            .filter(|e| e.uid != uid)
            .map(|e| e.to_list_entry())
            .collect();
        let list_json = serde_json::json!({ "players": list }).to_string();
        messages.push(dm(uid, 9001, list_json, 0));

        // 5. 给自己发背包 (5003)
        messages.push(dm(uid, 5003, player.to_inventory_json(), 1));

        // 6. 给自己发装备 (5004)
        messages.push(dm(uid, 5004, player.to_equipment_json(), 1));

        // 7. 技能数据 (通过 5004 下发，不再占用 5500 避免与升级消息冲突)
        // 5004 上行是装备，下行复用为技能列表
        // messages.push(dm(uid, 5004, player.to_skills_json(), 1));

        // 8. 给自己发任务列表 (5005)
        messages.push(dm(uid, 5005, player.to_quests_json(), 1));

        // 9. 给自己发NPC和怪物列表 (9002)
        let npcs_json: Vec<String> = self.state.npcs.iter().map(|n| n.to_json()).collect();
        let mobs_json: Vec<String> = self.state.mobs.iter().map(|m| m.to_list_entry()).collect();
        let entity_json = serde_json::json!({
            "npcs": npcs_json,
            "mobs": mobs_json,
        })
        .to_string();
        messages.push(dm(uid, 9002, entity_json, 0));

        self.state.players.insert(uid, player);

        Ok(Response::new(PlayerOnlineResponse {
            ok: true,
            messages,
        }))
    }

    async fn player_offline(
        &self,
        request: Request<PlayerOfflineRequest>,
    ) -> Result<Response<PlayerOfflineResponse>, Status> {
        let req = request.into_inner();
        let uid = req.player_uid;
        let count = self.state.online_count
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed)
            .saturating_sub(1);

        println!(
            "[LogicServer] 玩家离线: uid={} session={} reason={} (在线: {})",
            uid, req.session_id, req.reason, count
        );

        // 保存到 DB
        if let (Some(ref db), Some(p)) = (&self.db, self.state.players.get(&uid)) {
            let data = serde_json::json!({
                "uid": p.uid, "name": p.name, "level": p.level, "exp": p.exp,
                "x": p.x, "y": p.y, "hp": p.hp, "maxHp": p.max_hp,
                "mp": p.mp, "maxMp": p.max_mp, "atk": p.atk, "def": p.def,
                "weapon": p.weapon, "armor": p.armor, "accessory": p.accessory,
                "inventory": p.inventory.iter().map(|(id,c)| serde_json::json!({"itemId":id,"count":c})).collect::<Vec<_>>(),
                "quests": p.quests.iter().map(|(qid,pr)| serde_json::json!({"questId":qid,"progress":pr})).collect::<Vec<_>>(),
            });
            let _ = db.save_player(uid, &data).await;
        }

        self.state.players.remove(&uid);

        // 清除以该玩家为目标的怪物仇恨
        for mut mob in self.state.mobs.iter_mut() {
            if mob.target_uid == Some(uid) {
                mob.target_uid = None;
                mob.state = MobState::Idle;
            }
        }

        let leave_json = serde_json::json!({ "uid": uid }).to_string();

        Ok(Response::new(PlayerOfflineResponse {
            ok: true,
            messages: vec![dm(0, 8003, leave_json, 1)],
        }))
    }
}

impl GameState {
    fn process_message(&self, uid: u64, msg_id: u32, payload: &[u8]) -> ForwardResponse {
        let mut messages = Vec::new();
        let payload_str = String::from_utf8_lossy(payload);
        let json: Value = serde_json::from_str(&payload_str).unwrap_or(Value::Null);

        match msg_id {
            // ── 初始化/请求玩家列表 ──
            100 => {
                let list: Vec<String> = self
                    .players
                    .iter()
                    .filter(|e| e.uid != uid)
                    .map(|e| e.to_list_entry())
                    .collect();
                let list_json = serde_json::json!({ "players": list }).to_string();
                messages.push(dm(uid, 9001, list_json, 0));
            }

            // ── 战斗：基础攻击 ──
            1001 => {
                let target_uid = json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0);
                // ── 反外挂: 攻击频率校验 ──
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                if let Some(mut player) = self.players.get_mut(&uid) {
                    let elapsed = now.saturating_sub(player.last_attack_ms);
                    if elapsed < 400 {  // 普攻CD 800ms, 允许 400ms 误差
                        player.violation_count += 1;
                        tracing::warn!("反外挂: 攻击频率异常 uid={} elapsed={}ms viol={}", uid, elapsed, player.violation_count);
                        return ForwardResponse { messages: vec![] };
                    }
                    player.last_attack_ms = now;
                }
                messages.extend(self.handle_attack(uid, 1, target_uid));
            }

            // ── 战斗：技能攻击 ──
            1002 => {
                let skill_id = json.get("skillId").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                let target_uid = json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0);
                // ── 反外挂: 攻击频率校验 ──
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                if let Some(mut player) = self.players.get_mut(&uid) {
                    let elapsed = now.saturating_sub(player.last_attack_ms);
                    if elapsed < 400 {
                        player.violation_count += 1;
                        tracing::warn!("反外挂: 技能频率异常 uid={} elapsed={}ms viol={}", uid, elapsed, player.violation_count);
                        return ForwardResponse { messages: vec![] };
                    }
                    player.last_attack_ms = now;
                }
                messages.extend(self.handle_attack(uid, skill_id, target_uid));
            }

            // ── 拾取物品 ──
            1003 => {
                let drop_id = json.get("dropId").and_then(|v| v.as_u64()).unwrap_or(0);
                messages.extend(self.handle_pickup(uid, drop_id));
            }

            // ── 装备/卸下 ──
            1004 => {
                let item_id = json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                // ── 反外挂: 背包校验 ──
                if let Some(player) = self.players.get(&uid) {
                    if item_id != 0 && !player.inventory.iter().any(|(id, c)| *id == item_id && *c > 0) {
                        tracing::warn!("反外挂: 装备不存在的物品 uid={} item={}", uid, item_id);
                        return ForwardResponse { messages: vec![dm(uid, 5004, serde_json::json!({"error": "item_not_found"}).to_string(), 0)] };
                    }
                }
                messages.extend(self.handle_equip(uid, item_id));
            }

            // ── 接受任务 ──
            1005 => {
                let quest_id = json.get("questId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                messages.extend(self.handle_accept_quest(uid, quest_id));
            }

            // ── 完成任务 ──
            1006 => {
                let quest_id = json.get("questId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                messages.extend(self.handle_complete_quest(uid, quest_id));
            }

            // ── NPC交互 ──
            1007 => {
                let npc_id = json.get("npcId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                messages.extend(self.handle_npc_interact(uid, npc_id));
            }

            // ── 使用物品 ──
            1008 => {
                let item_id = json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                // ── 反外挂: 背包校验 ──
                if let Some(player) = self.players.get(&uid) {
                    if !player.inventory.iter().any(|(id, c)| *id == item_id && *c > 0) {
                        tracing::warn!("反外挂: 使用不存在的物品 uid={} item={}", uid, item_id);
                        return ForwardResponse { messages: vec![dm(uid, 6001, serde_json::json!({"error": "item_not_found"}).to_string(), 0)] };
                    }
                }
                messages.extend(self.handle_use_item(uid, item_id));
            }

            // ── 队伍邀请 ──
            2002 => {
                let target = json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0);
                if target > 0 {
                    let leader_name = self.players.get(&uid).map(|p| p.name.clone()).unwrap_or_else(|| format!("Player{}", uid));
                    let party_id = self.party_mgr.create_and_invite(uid, &leader_name, target);
                    if party_id > 0 {
                        let invite = serde_json::json!({"type":"party_invite","from":uid,"fromName":leader_name,"partyId":party_id}).to_string();
                        messages.push(dm(target, 7002, invite, 1));
                        let ack = serde_json::json!({"type":"party_created","partyId":party_id}).to_string();
                        messages.push(dm(uid, 7001, ack, 1));
                    }
                }
            }

            // ── 接受邀请 ──
            2003 => {
                let name = self.players.get(&uid).map(|p| p.name.clone()).unwrap_or_else(|| format!("Player{}", uid));
                if let Some(party_id) = self.party_mgr.accept_invite(uid, &name) {
                    let join = serde_json::json!({"type":"party_join","uid":uid,"name":name,"partyId":party_id}).to_string();
                    let members = self.party_mgr.get_party_members(party_id);
                    for m_uid in members {
                        if m_uid != uid {
                            messages.push(dm(0, 7002, join.clone(), 0));
                        }
                    }
                    let ack = serde_json::json!({"type":"party_joined","partyId":party_id}).to_string();
                    messages.push(dm(uid, 7001, ack, 1));
                }
            }

            // ── 离开队伍 ──
            2004 => {
                let name = self.players.get(&uid).map(|p| p.name.clone()).unwrap_or_else(|| format!("Player{}", uid));
                if let Some(party_id) = self.party_mgr.get_party_id(uid) {
                    let members = self.party_mgr.get_party_members(party_id);
                    self.party_mgr.leave(uid);
                    let leave = serde_json::json!({"type":"party_leave","uid":uid,"name":name}).to_string();
                    for m_uid in members {
                        if m_uid != uid {
                            messages.push(dm(0, 7002, leave.clone(), 0));
                        }
                    }
                    let ack = serde_json::json!({"type":"party_left"}).to_string();
                    messages.push(dm(uid, 7001, ack, 1));
                }
            }

            // ── 聊天 ──
            2000..=2999 => {
                let text = json
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&payload_str)
                    .to_string();

                let from_name = self
                    .players
                    .get(&uid)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| format!("Player{}", uid));

                let ack_json = serde_json::json!({ "msgId": msg_id }).to_string();
                messages.push(dm(uid, 7001, ack_json, 1));

                let broadcast_json = serde_json::json!({
                    "from": uid,
                    "fromName": from_name,
                    "text": text,
                })
                .to_string();
                messages.push(dm(0, 7002, broadcast_json, 1));
            }

            // ── 移动 ──
            3000..=3999 => {
                let x = json.get("x").and_then(|v| v.as_f64()).unwrap_or(400.0) as f32;
                let y = json.get("y").and_then(|v| v.as_f64()).unwrap_or(300.0) as f32;
                let dir = json.get("dir").and_then(|v| v.as_u64()).unwrap_or(2) as u8;

                if let Some(mut player) = self.players.get_mut(&uid) {
                    // ── 反外挂: 移动速度校验 ──
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                    let dx = x - player.last_x;
                    let dy = y - player.last_y;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let dt = if player.last_move_ms > 0 { now.saturating_sub(player.last_move_ms) } else { 100 };
                    // 速度阈值: 200 单位/秒 (正常客户端约 60/s, 留 3x 余量)
                    let max_dist = (dt as f32 / 1000.0) * 200.0;
                    if dist > max_dist && player.last_move_ms > 0 {
                        player.violation_count += 1;
                        tracing::warn!("反外挂: 移动速度异常 uid={} dist={} max={} viol={}", uid, dist, max_dist, player.violation_count);
                        // 强制拉回到上次合法位置
                        return ForwardResponse { messages: vec![dm(uid, 5001, serde_json::json!({
                            "uid": uid, "x": player.last_x, "y": player.last_y,
                            "hp": player.hp, "maxHp": player.max_hp,
                            "mp": player.mp, "maxMp": player.max_mp
                        }).to_string(), 0)] };
                    }
                    player.last_x = x;
                    player.last_y = y;
                    player.last_move_ms = now;
                    player.x = x;
                    player.y = y;
                    player.dir = dir;
                }

                let pos_json = serde_json::json!({
                    "uid": uid,
                    "x": x,
                    "y": y,
                    "dir": dir,
                })
                .to_string();
                messages.push(dm(0, 8001, pos_json, 0));
            }

            // ── 查询附近玩家 ──
            4001 => {
                let list: Vec<String> = self
                    .players
                    .iter()
                    .filter(|e| e.uid != uid)
                    .map(|e| e.to_list_entry())
                    .collect();
                let list_json = serde_json::json!({ "players": list }).to_string();
                messages.push(dm(uid, 9001, list_json, 0));
            }

            // ── 查询附近实体(NPC/怪物) + 触发怪物AI ──
            4002 => {
                // 每次查询时运行一次怪物AI tick
                self.tick_mob_ai(uid);

                let npcs_json: Vec<String> = self.npcs.iter().map(|n| n.to_json()).collect();
                let mobs_json: Vec<String> = self.mobs.iter().map(|m| m.to_list_entry()).collect();
                let entity_json = serde_json::json!({
                    "npcs": npcs_json,
                    "mobs": mobs_json,
                })
                .to_string();
                messages.push(dm(uid, 9002, entity_json, 0));
            }

            _ => {
                let echo_json = serde_json::json!({
                    "type": "echo",
                    "uid": uid,
                    "msg_id": msg_id,
                    "data": &payload_str,
                })
                .to_string();
                messages.push(dm(uid, msg_id + 5000, echo_json, 0));
            }
        }

        // 附带怪物位置(后台 loop 已 tick，此处仅广播最新位置)
        for mob in self.mobs.iter() {
            if mob.state != MobState::Dead {
                messages.push(dm(0, 8004, serde_json::json!({
                    "entityId": mob.entity_id, "x": mob.x, "y": mob.y,
                    "hp": mob.hp, "maxHp": mob.max_hp,
                }).to_string(), 0));
            }
        }

        // 附带玩家最新 HP/MP（怪物可能已攻击）
        if let Some(p) = self.players.get(&uid) {
            messages.push(dm(uid, 5001, p.to_stats_json(), 1));
        }

        ForwardResponse { messages }
    }

    // ════════════════════════════════════════════════════════════
    // 战斗处理
    // ════════════════════════════════════════════════════════════
    fn handle_attack(&self, uid: u64, skill_id: u32, target_uid: u64) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();
        let now = current_millis();

        let skill = match get_skill_def(skill_id) {
            Some(s) => s,
            None => {
                let err = serde_json::json!({ "error": "invalid_skill" }).to_string();
                messages.push(dm(uid, 6001, err, 2));
                return messages;
            }
        };

        // 检查冷却
        let mut player = match self.players.get_mut(&uid) {
            Some(p) => p,
            None => return messages,
        };

        if let Some(&last_cast) = player.skill_cooldowns.get(&skill_id) {
            if now - last_cast < skill.cooldown_ms {
                let cd_left = skill.cooldown_ms - (now - last_cast);
                let err = serde_json::json!({
                    "error": "cooldown",
                    "skillId": skill_id,
                    "cooldownLeft": cd_left,
                })
                .to_string();
                messages.push(dm(uid, 6001, err, 2));
                return messages;
            }
        }

        // 检查MP
        if player.mp < skill.mp_cost {
            let err = serde_json::json!({ "error": "not_enough_mp" }).to_string();
            messages.push(dm(uid, 6001, err, 2));
            return messages;
        }

        player.mp -= skill.mp_cost;
        player.skill_cooldowns.insert(skill_id, now);

        let player_atk = player.total_atk();
        let player_x = player.x;
        let player_y = player.y;
        let _player_level = player.level;
        drop(player);

        // 更新技能冷却信息
        messages.push(dm(uid, 5500, {
            if let Some(p) = self.players.get(&uid) {
                p.to_skills_json()
            } else {
                "{}".to_string()
            }
        }, 1));

        // 更新MP
        let mp_json = serde_json::json!({
            "mp": self.players.get(&uid).map(|p| p.mp).unwrap_or(0),
            "maxMp": self.players.get(&uid).map(|p| p.max_mp).unwrap_or(50),
        }).to_string();
        messages.push(dm(uid, 5002, mp_json, 1));

        // 目标是怪物实体 (先查 mobs 表)
        if target_uid >= 10000 && self.mobs.contains_key(&target_uid) {
            let mut mob = match self.mobs.get_mut(&target_uid) {
                Some(m) => m,
                None => {
                    let miss = serde_json::json!({
                        "targetUid": target_uid,
                        "dmg": 0,
                        "targetHp": 0,
                        "miss": true,
                    }).to_string();
                    messages.push(dm(uid, 6001, miss, 2));
                    return messages;
                }
            };

            if mob.state == MobState::Dead {
                let miss = serde_json::json!({
                    "targetUid": target_uid,
                    "dmg": 0,
                    "targetHp": 0,
                    "miss": true,
                }).to_string();
                messages.push(dm(uid, 6001, miss, 2));
                return messages;
            }

            // 检查距离
            let dist = distance(player_x, player_y, mob.x, mob.y);
            if dist > skill.range + 20.0 {
                let miss = serde_json::json!({
                    "targetUid": target_uid,
                    "dmg": 0,
                    "targetHp": mob.hp,
                    "miss": true,
                    "reason": "out_of_range",
                }).to_string();
                messages.push(dm(uid, 6001, miss, 2));
                return messages;
            }

            // 计算伤害
            let base_dmg = (player_atk as f32 * skill.dmg_multiplier) as i32;
            let dmg = (base_dmg - mob.def).max(1);
            let crit = (uid + now) % 5 == 0; // 20% 暴击
            let final_dmg = if crit { dmg * 2 } else { dmg };

            mob.hp = (mob.hp - final_dmg).max(0);
            mob.target_uid = Some(uid);
            mob.state = MobState::Chasing;
            let mob_hp = mob.hp;
            let mob_x = mob.x;
            let mob_y = mob.y;
            let mob_def_id = mob.def_id;
            let mob_exp = mob.exp;
            let mob_name = mob.name.clone();

            // 给攻击者发战斗结果
            let battle_json = serde_json::json!({
                "targetUid": target_uid,
                "targetName": mob_name,
                "dmg": final_dmg,
                "targetHp": mob_hp,
                "crit": crit,
                "skillId": skill_id,
            }).to_string();
            messages.push(dm(uid, 6001, battle_json, 2));

            // 广播实体HP更新
            let mob_state_json = serde_json::json!({
                "entityId": target_uid,
                "hp": mob_hp,
                "maxHp": mob.max_hp,
                "state": format!("{:?}", MobState::Chasing),
                "x": mob_x,
                "y": mob_y,
            }).to_string();
            messages.push(dm(0, 6002, mob_state_json, 1));

            // 怪物死亡
            if mob_hp == 0 {
                mob.state = MobState::Dead;

                // 广播死亡信息 + 掉落
                let drops = self.generate_drops(mob_def_id, mob_x, mob_y);
                let drop_json: Vec<String> = drops.iter().map(|d| d.to_json()).collect();

                let death_json = serde_json::json!({
                    "entityId": target_uid,
                    "killer": uid,
                    "killerName": format!("Player{}", uid),
                    "mobName": mob_name,
                    "drops": drop_json,
                    "exp": mob_exp,
                }).to_string();
                messages.push(dm(0, 6003, death_json, 1));

                // 插入掉落物
                for drop in &drops {
                    self.drops.insert(drop.drop_id, drop.clone());
                }

                // 给击杀者加经验
                if let Some(mut p) = self.players.get_mut(&uid) {
                    let _old_level = p.level;
                    let leveled_up = p.add_exp(mob_exp);

                    // 更新任务进度
                    let quest_updated = p.update_quest_progress(mob_def_id);

                    let exp_json = serde_json::json!({
                        "exp": p.exp,
                        "maxExp": exp_for_level(p.level),
                        "level": p.level,
                        "gained": mob_exp,
                    }).to_string();
                    messages.push(dm(uid, 5002, exp_json, 1));

                    if leveled_up {
                        let levelup_json = serde_json::json!({
                            "level": p.level,
                            "maxHp": p.max_hp,
                            "maxMp": p.max_mp,
                            "hp": p.hp,
                            "mp": p.mp,
                            "atk": p.total_atk(),
                            "def": p.total_def(),
                        }).to_string();
                        messages.push(dm(uid, 5001, levelup_json, 2));

                        let broadcast = serde_json::json!({
                            "from": 0,
                            "fromName": "System",
                            "text": format!("Player{} 升到了 {} 级!", uid, p.level),
                        }).to_string();
                        messages.push(dm(0, 7002, broadcast, 1));
                    }

                    if quest_updated {
                        messages.push(dm(uid, 5005, p.to_quests_json(), 1));
                    }
                }

                // 安排复活 (在 process_tick 中处理)
                // 设置死亡时间
                mob.last_attack = now; // reuse as death time
            }

            return messages;
        }

        // 目标是玩家 (查 players 表, 不限 UID 范围)
        if target_uid > 0 && self.players.contains_key(&target_uid) {
            let mut target = match self.players.get_mut(&target_uid) {
                Some(t) => t,
                None => {
                    let miss = serde_json::json!({
                        "targetUid": target_uid,
                        "dmg": 0,
                        "targetHp": 0,
                        "miss": true,
                    }).to_string();
                    messages.push(dm(uid, 6001, miss, 2));
                    return messages;
                }
            };

            let dist = distance(player_x, player_y, target.x, target.y);
            if dist > skill.range + 20.0 {
                let miss = serde_json::json!({
                    "targetUid": target_uid,
                    "dmg": 0,
                    "targetHp": target.hp,
                    "miss": true,
                    "reason": "out_of_range",
                }).to_string();
                messages.push(dm(uid, 6001, miss, 2));
                return messages;
            }

            let base_dmg = (player_atk as f32 * skill.dmg_multiplier) as i32;
            let dmg = (base_dmg - target.total_def()).max(1);
            let crit = (uid + now) % 5 == 0;
            let final_dmg = if crit { dmg * 2 } else { dmg };

            target.hp = (target.hp - final_dmg).max(0);
            let target_hp = target.hp;
            let target_max_hp = target.max_hp;

            let battle_json = serde_json::json!({
                "targetUid": target_uid,
                "targetName": target.name,
                "dmg": final_dmg,
                "targetHp": target_hp,
                "crit": crit,
                "skillId": skill_id,
            }).to_string();
            messages.push(dm(uid, 6001, battle_json, 2));

            let hit_json = serde_json::json!({
                "attackerUid": uid,
                "attackerName": format!("Player{}", uid),
                "dmg": final_dmg,
                "hp": target_hp,
                "maxHp": target_max_hp,
                "crit": crit,
            }).to_string();
            messages.push(dm(target_uid, 6001, hit_json, 2));

            if target_hp == 0 {
                let kill_json = serde_json::json!({
                    "from": 0,
                    "fromName": "System",
                    "text": format!("Player{} 击杀了 Player{}!", uid, target_uid),
                }).to_string();
                messages.push(dm(0, 7002, kill_json, 1));

                // 自动复活
                if let Some(mut t) = self.players.get_mut(&target_uid) {
                    t.hp = t.max_hp;
                    t.mp = t.max_mp;
                    let revive_json = serde_json::json!({
                        "hp": t.hp,
                        "maxHp": t.max_hp,
                        "mp": t.mp,
                        "maxMp": t.max_mp,
                        "revived": true,
                    }).to_string();
                    messages.push(dm(target_uid, 5001, revive_json, 2));
                }
            }

            return messages;
        }

        // 无目标 - 空挥
        let echo_json = serde_json::json!({
            "uid": uid,
            "dmg": (player_atk as f32 * skill.dmg_multiplier) as i32,
            "skillId": skill_id,
            "swing": true,
        }).to_string();
        messages.push(dm(uid, 6001, echo_json, 2));

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 拾取物品
    // ════════════════════════════════════════════════════════════
    fn handle_pickup(&self, uid: u64, drop_id: u64) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let drop = match self.drops.get(&drop_id) {
            Some(d) => d.clone(),
            None => {
                let err = serde_json::json!({ "error": "item_not_found" }).to_string();
                messages.push(dm(uid, 5003, err, 2));
                return messages;
            }
        };

        // 检查距离
        let player_pos = self.players.get(&uid).map(|p| (p.x, p.y));
        if let Some((px, py)) = player_pos {
            if distance(px, py, drop.x, drop.y) > 60.0 {
                let err = serde_json::json!({ "error": "too_far" }).to_string();
                messages.push(dm(uid, 5003, err, 2));
                return messages;
            }
        }

        self.drops.remove(&drop_id);

        // 添加到背包
        if let Some(mut p) = self.players.get_mut(&uid) {
            p.add_item(drop.item_id, drop.count);
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
        }

        // 广播掉落物被拾取
        let pickup_json = serde_json::json!({
            "dropId": drop_id,
            "pickedBy": uid,
        }).to_string();
        messages.push(dm(0, 6003, pickup_json, 1));

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 装备/卸下
    // ════════════════════════════════════════════════════════════
    fn handle_equip(&self, uid: u64, item_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let item = match get_item_def(item_id) {
            Some(i) => i,
            None => return messages,
        };

        if item.item_type == "potion" || item.item_type == "material" {
            let err = serde_json::json!({ "error": "cannot_equip" }).to_string();
            messages.push(dm(uid, 5004, err, 2));
            return messages;
        }

        if let Some(mut p) = self.players.get_mut(&uid) {
            // 检查背包是否有该物品
            if !p.inventory.iter().any(|(id, c)| *id == item_id && *c > 0) {
                let err = serde_json::json!({ "error": "not_in_inventory" }).to_string();
                messages.push(dm(uid, 5004, err, 2));
                return messages;
            }

            let slot = match item.item_type {
                "weapon" => &mut p.weapon,
                "armor" => &mut p.armor,
                "accessory" => &mut p.accessory,
                _ => return messages,
            };

            // 交换装备：旧的放回背包，新的装备上
            let old = *slot;
            *slot = Some(item_id);

            // 从背包移除新装备的物品
            if let Some(entry) = p.inventory.iter_mut().find(|(id, _)| *id == item_id) {
                entry.1 -= 1;
            }

            // 旧装备放回背包
            if let Some(old_id) = old {
                p.add_item(old_id, 1);
            }

            // 发送更新
            messages.push(dm(uid, 5004, p.to_equipment_json(), 1));
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(dm(uid, 5001, p.to_stats_json(), 1));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 接受任务
    // ════════════════════════════════════════════════════════════
    fn handle_accept_quest(&self, uid: u64, quest_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let def = match get_quest_def(quest_id) {
            Some(d) => d,
            None => return messages,
        };

        if let Some(mut p) = self.players.get_mut(&uid) {
            // 检查是否已接受
            if p.quests.iter().any(|(qid, _)| *qid == quest_id) {
                let err = serde_json::json!({ "error": "quest_already_accepted" }).to_string();
                messages.push(dm(uid, 5005, err, 2));
                return messages;
            }

            p.quests.push((quest_id, 0));
            messages.push(dm(uid, 5005, p.to_quests_json(), 1));

            let sys_msg = serde_json::json!({
                "from": 0,
                "fromName": "System",
                "text": format!("接受任务: {}", def.name),
            }).to_string();
            messages.push(dm(uid, 7002, sys_msg, 1));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 完成任务
    // ════════════════════════════════════════════════════════════
    fn handle_complete_quest(&self, uid: u64, quest_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let def = match get_quest_def(quest_id) {
            Some(d) => d,
            None => return messages,
        };

        if let Some(mut p) = self.players.get_mut(&uid) {
            // 查找任务进度
            let progress = p.quests.iter().find(|(qid, _)| *qid == quest_id).map(|(_, c)| *c);
            let progress = match progress {
                Some(c) => c,
                None => {
                    let err = serde_json::json!({ "error": "quest_not_accepted" }).to_string();
                    messages.push(dm(uid, 5005, err, 2));
                    return messages;
                }
            };

            if progress < def.target_count {
                let err = serde_json::json!({
                    "error": "quest_not_complete",
                    "progress": progress,
                    "target": def.target_count,
                }).to_string();
                messages.push(dm(uid, 5005, err, 2));
                return messages;
            }

            // 完成任务：移除任务，给奖励
            p.quests.retain(|(qid, _)| *qid != quest_id);

            // 经验奖励
            let _old_level = p.level;
            let leveled_up = p.add_exp(def.exp_reward);

            // 物品奖励
            p.add_item(def.item_reward, 1);

            messages.push(dm(uid, 5005, p.to_quests_json(), 1));
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));

            let exp_json = serde_json::json!({
                "exp": p.exp,
                "maxExp": exp_for_level(p.level),
                "level": p.level,
                "gained": def.exp_reward,
            }).to_string();
            messages.push(dm(uid, 5002, exp_json, 1));

            if leveled_up {
                messages.push(dm(uid, 5001, p.to_stats_json(), 2));
                let broadcast = serde_json::json!({
                    "from": 0,
                    "fromName": "System",
                    "text": format!("Player{} 升到了 {} 级!", uid, p.level),
                }).to_string();
                messages.push(dm(0, 7002, broadcast, 1));
            }

            let sys_msg = serde_json::json!({
                "from": 0,
                "fromName": "System",
                "text": format!("完成任务: {}! 获得经验{} 点", def.name, def.exp_reward),
            }).to_string();
            messages.push(dm(uid, 7002, sys_msg, 1));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // NPC交互
    // ════════════════════════════════════════════════════════════
    fn handle_npc_interact(&self, uid: u64, npc_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let npc = match self.npcs.iter().find(|n| n.id == npc_id) {
            Some(n) => n,
            None => return messages,
        };

        let mut options: Vec<Value> = Vec::new();

        match npc.npc_type.as_str() {
            "quest_giver" => {
                // 显示可接任务
                let available_quests: Vec<&QuestDef> = QUEST_DEFS
                    .iter()
                    .filter(|q| {
                        if let Some(p) = self.players.get(&uid) {
                            !p.quests.iter().any(|(qid, _)| *qid == q.id)
                        } else {
                            true
                        }
                    })
                    .collect();

                for q in available_quests {
                    options.push(serde_json::json!({
                        "type": "accept_quest",
                        "questId": q.id,
                        "label": format!("接受任务: {}", q.name),
                    }));
                }

                // 显示可完成任务
                if let Some(p) = self.players.get(&uid) {
                    for (qid, progress) in &p.quests {
                        if let Some(def) = get_quest_def(*qid) {
                            if *progress >= def.target_count {
                                options.push(serde_json::json!({
                                    "type": "complete_quest",
                                    "questId": qid,
                                    "label": format!("完成任务: {}", def.name),
                                }));
                            }
                        }
                    }
                }
            }
            "healer" => {
                options.push(serde_json::json!({
                    "type": "heal",
                    "label": "完全恢复 (免费)",
                }));
            }
            "merchant" => {
                options.push(serde_json::json!({
                    "type": "shop",
                    "label": "查看商品",
                }));
            }
            _ => {}
        }

        let dialog_json = serde_json::json!({
            "npcId": npc.id,
            "name": npc.name,
            "dialog": npc.dialog,
            "type": npc.npc_type,
            "options": options,
        })
        .to_string();
        messages.push(dm(uid, 5006, dialog_json, 1));

        // 治疗师直接治疗
        if npc.npc_type == "healer" {
            if let Some(mut p) = self.players.get_mut(&uid) {
                p.hp = p.max_hp;
                p.mp = p.max_mp;
            }
            let heal_json = serde_json::json!({
                "hp": self.players.get(&uid).map(|p| p.hp).unwrap_or(100),
                "maxHp": self.players.get(&uid).map(|p| p.max_hp).unwrap_or(100),
                "mp": self.players.get(&uid).map(|p| p.mp).unwrap_or(50),
                "maxMp": self.players.get(&uid).map(|p| p.max_mp).unwrap_or(50),
                "healed": true,
            }).to_string();
            messages.push(dm(uid, 5001, heal_json, 2));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 使用物品
    // ════════════════════════════════════════════════════════════
    fn handle_use_item(&self, uid: u64, item_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let item = match get_item_def(item_id) {
            Some(i) => i,
            None => return messages,
        };

        if item.item_type != "potion" {
            let err = serde_json::json!({ "error": "cannot_use" }).to_string();
            messages.push(dm(uid, 5003, err, 2));
            return messages;
        }

        if let Some(mut p) = self.players.get_mut(&uid) {
            if !p.remove_item(item_id, 1) {
                let err = serde_json::json!({ "error": "not_in_inventory" }).to_string();
                messages.push(dm(uid, 5003, err, 2));
                return messages;
            }

            if item.hp_restore > 0 {
                p.hp = (p.hp + item.hp_restore).min(p.max_hp);
            }
            if item.mp_restore > 0 {
                p.mp = (p.mp + item.mp_restore).min(p.max_mp);
            }

            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(dm(uid, 5001, p.to_stats_json(), 1));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 生成掉落物
    // ════════════════════════════════════════════════════════════
    fn generate_drops(&self, mob_def_id: u32, x: f32, y: f32) -> Vec<ItemDrop> {
        let mut drops = Vec::new();
        let now = current_millis();

        match mob_def_id {
            1 => { // 史莱姆
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 9, x: x + 10.0, y: y + 10.0, count: 1 });
                if now % 3 == 0 {
                    let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    drops.push(ItemDrop { drop_id, item_id: 6, x: x - 10.0, y: y + 5.0, count: 1 });
                }
            }
            2 => { // 哥布林
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 10, x: x + 10.0, y: y + 10.0, count: 1 });
                if now % 2 == 0 {
                    let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    drops.push(ItemDrop { drop_id, item_id: 7, x: x - 10.0, y: y + 5.0, count: 1 });
                }
            }
            3 => { // 骷髅战士
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 1, x: x + 10.0, y: y + 10.0, count: 1 });
                if now % 2 == 0 {
                    let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    drops.push(ItemDrop { drop_id, item_id: 6, x: x - 10.0, y: y + 5.0, count: 1 });
                }
            }
            4 => { // 暗影法师
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 2, x: x + 10.0, y: y + 10.0, count: 1 });
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x - 10.0, y: y + 5.0, count: 1 });
            }
            5 => { // 岩石巨人
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 4, x: x + 10.0, y: y + 10.0, count: 1 });
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 5, x: x - 10.0, y: y + 5.0, count: 1 });
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x + 5.0, y: y - 10.0, count: 1 });
            }
            _ => {}
        }

        drops
    }

    // ════════════════════════════════════════════════════════════
    // 怪物AI Tick (同步，由查询触发)
    // ════════════════════════════════════════════════════════════
    fn tick_mob_ai(&self, _querying_uid: u64) {
        let now = current_millis();

        // 收集所有玩家位置
        let player_positions: Vec<(u64, f32, f32)> = self
            .players
            .iter()
            .map(|p| (p.uid, p.x, p.y))
            .collect();

        for mut mob in self.mobs.iter_mut() {
            let def = match get_mob_def(mob.def_id) {
                Some(d) => d,
                None => continue,
            };

            match mob.state {
                MobState::Dead => {
                    // 5秒后复活
                    if now - mob.last_attack > 5000 {
                        mob.hp = mob.max_hp;
                        mob.state = MobState::Idle;
                        mob.x = mob.spawn_x;
                        mob.y = mob.spawn_y;
                        mob.target_uid = None;
                        mob.patrol_tx = None;
                        mob.patrol_ty = None;
                    }
                }
                MobState::Idle | MobState::Patrolling => {
                    // 检测附近玩家
                    let mut nearest_player: Option<(u64, f32, f32, f32)> = None;
                    for (puid, px, py) in &player_positions {
                        let dist = distance(mob.x, mob.y, *px, *py);
                        if dist < def.detect_range {
                            if nearest_player.is_none() || dist < nearest_player.unwrap().3 {
                                nearest_player = Some((*puid, *px, *py, dist));
                            }
                        }
                    }

                    if let Some((puid, _, _, _)) = nearest_player {
                        mob.target_uid = Some(puid);
                        mob.state = MobState::Chasing;
                    } else {
                        // 巡逻：小步长移动到出生点周围的随机位置
                        if now - mob.last_move > 500 {
                            mob.last_move = now;
                            mob.move_dir = (now % 628) as f32 / 100.0;
                            mob.patrol_tx = Some((mob.spawn_x + (mob.move_dir.cos() * def.radius))
                                .max(20.0).min(WORLD_W - 20.0));
                            mob.patrol_ty = Some((mob.spawn_y + (mob.move_dir.sin() * def.radius))
                                .max(20.0).min(WORLD_H - 20.0));
                        }
                        // 每帧小步移动
                        if let (Some(tx), Some(ty)) = (mob.patrol_tx, mob.patrol_ty) {
                            let dx = tx - mob.x;
                            let dy = ty - mob.y;
                            let len = (dx*dx+dy*dy).sqrt();
                            if len > 2.0 {
                                mob.x += (dx/len) * def.move_speed * 3.0;
                                mob.y += (dy/len) * def.move_speed * 3.0;
                                mob.dir = if dx > 0.0 { 1 } else { 3 };
                            }
                        }
                    }
                }
                MobState::Chasing => {
                    if let Some(target_uid) = mob.target_uid {
                        let target_pos = player_positions.iter()
                            .find(|(uid, _, _)| *uid == target_uid);

                        if let Some((_, px, py)) = target_pos {
                            let dist = distance(mob.x, mob.y, *px, *py);

                            if dist > def.detect_range * 2.0 {
                                mob.target_uid = None;
                                mob.state = MobState::Idle;
                            } else if dist <= def.attack_range {
                                mob.state = MobState::Attacking;
                                if now - mob.last_attack > def.attack_cd_ms {
                                    mob.last_attack = now;

                                    // 对玩家造成伤害 (直接修改，玩家下次查询时会看到HP变化)
                                    if let Some(mut target) = self.players.get_mut(&target_uid) {
                                        let dmg = (mob.atk - target.total_def()).max(1);
                                        target.hp = (target.hp - dmg).max(0);
                                        if target.hp == 0 {
                                            // 自动复活
                                            target.hp = target.max_hp;
                                            target.mp = target.max_mp;
                                        }
                                    }
                                }
                            } else {
                                // 追击移动
                                let dx = *px - mob.x;
                                let dy = *py - mob.y;
                                let len = (dx * dx + dy * dy).sqrt();
                                if len > 0.0 {
                                    mob.x += (dx / len) * def.move_speed * 2.0;
                                    mob.y += (dy / len) * def.move_speed * 2.0;
                                    mob.dir = if dx > 0.0 { 1 } else { 3 };
                                }
                            }
                        } else {
                            mob.target_uid = None;
                            mob.state = MobState::Idle;
                        }
                    } else {
                        mob.state = MobState::Idle;
                    }
                }
                MobState::Attacking => {
                    mob.state = MobState::Chasing;
                }
                MobState::Respawning => {
                    mob.state = MobState::Idle;
                }
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════
// Main
// ════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "0.0.0.0:50051".parse()?;

    println!("═══════════════════════════════════════════");
    println!("  Mock MMORPG 逻辑服 (完整版) 启动中...");
    println!("  gRPC 监听: {}", addr);
    println!("  世界尺寸: {}x{}", WORLD_W, WORLD_H);
    println!("═══════════════════════════════════════════");
    println!();
    println!("消息协议:");
    println!("  上行: 100=初始化 1001=攻击 1002=技能 1003=拾取 1004=装备");
    println!("        1005=接任务 1006=完成任务 1007=NPC交互 1008=使用物品");
    println!("        2001=聊天 3001=移动 4001=查询玩家 4002=查询实体");
    println!("  下行: 5001=属性 5002=经验 5003=背包 5004=装备 5005=任务");
    println!("        5006=NPC对话 5500=技能列表");
    println!("        6001=战斗 6002=实体状态 6003=死亡/掉落");
    println!("        7001=聊天ACK 7002=聊天广播");
    println!("        8001=位置 8002=进入 8003=离开 8004=实体位置");
    println!("        9001=玩家列表 9002=实体列表");
    println!();
    println!("游戏内容:");
    println!("  NPC: {} 个", NPC_DEFS.len());
    println!("  怪物: {} 种, {} 个实例", MOB_DEFS.len(), MOB_DEFS.len() * 3);
    println!("  物品: {} 种", ITEM_DEFS.len());
    println!("  任务: {} 个", QUEST_DEFS.len());
    println!("  技能: {} 个", SKILLS.len());
    println!();

    let mut service = MockLogicService::default();

    // 尝试连接 PostgreSQL（不可用则降级，游戏仍正常运行）
    match logic_lib::db::Database::new("postgres://mmo:mmo_dev_pass@127.0.0.1:5433/mmorpg").await {
        Ok(db) => {
            println!("  ✅ PostgreSQL 已连接");
            let db = Arc::new(db);
            service.db = Some(db.clone());

            // 后台自动存盘 — 每 30 秒
            let state = service.state.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                    for player in state.players.iter() {
                        let data = serde_json::json!({
                            "uid": player.uid,
                            "name": player.name,
                            "level": player.level,
                            "exp": player.exp,
                            "x": player.x, "y": player.y,
                            "hp": player.hp, "maxHp": player.max_hp,
                            "mp": player.mp, "maxMp": player.max_mp,
                            "atk": player.atk, "def": player.def,
                            "weapon": player.weapon, "armor": player.armor, "accessory": player.accessory,
                            "inventory": player.inventory.iter().map(|(id,c)| serde_json::json!({"itemId":id,"count":c})).collect::<Vec<_>>(),
                            "quests": player.quests.iter().map(|(qid,p)| serde_json::json!({"questId":qid,"progress":p})).collect::<Vec<_>>(),
                        });
                        let _ = db.save_player(player.uid, &data).await;
                    }
                }
            });
        }
        Err(e) => {
            println!("  ⚠️ PostgreSQL 不可用 ({}): 数据仅存内存", e);
        }
    }

    Server::builder()
        .add_service(LogicServiceServer::new(service))
        .serve(addr)
        .await?;

    println!("逻辑服已停止");
    Ok(())
}
