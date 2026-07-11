// ════════════════════════════════════════════════════════════════
// 常量定义
// ════════════════════════════════════════════════════════════════

/// 世界尺寸
pub const WORLD_W: f32 = 1600.0;
pub const WORLD_H: f32 = 1200.0;

/// 技能定义
pub struct SkillDef {
    pub id: u32,
    pub name: &'static str,
    pub dmg_multiplier: f32,
    pub mp_cost: i32,
    pub cooldown_ms: u64,
    pub range: f32,
    pub icon: &'static str,
}

pub const SKILLS: &[SkillDef] = &[
    SkillDef { id: 1, name: "普通攻击", dmg_multiplier: 1.0,  mp_cost: 0,  cooldown_ms: 800,  range: 80.0,  icon: "⚔" },
    SkillDef { id: 2, name: "重击",     dmg_multiplier: 2.0,  mp_cost: 10, cooldown_ms: 2000, range: 80.0,  icon: "💥" },
    SkillDef { id: 3, name: "火球术",   dmg_multiplier: 3.0,  mp_cost: 20, cooldown_ms: 3000, range: 200.0, icon: "🔥" },
    SkillDef { id: 4, name: "冰冻",     dmg_multiplier: 1.5,  mp_cost: 15, cooldown_ms: 4000, range: 150.0, icon: "❄" },
    SkillDef { id: 5, name: "治疗术",   dmg_multiplier: 0.0,  mp_cost: 25, cooldown_ms: 5000, range: 0.0,   icon: "💚" },
];

// ── 职业与天赋 (v0.6) ──
pub struct ClassDef {
    pub id: u8, pub name: &'static str, pub icon: &'static str,
    pub atk_bonus: i32, pub def_bonus: i32, pub hp_bonus: i32,
}
pub const CLASS_DEFS: &[ClassDef] = &[
    ClassDef { id: 1, name: "战士", icon: "⚔", atk_bonus: 5, def_bonus: 10, hp_bonus: 50 },
    ClassDef { id: 2, name: "法师", icon: "🔮", atk_bonus: 15, def_bonus: 2, hp_bonus: -20 },
    ClassDef { id: 3, name: "弓手", icon: "🏹", atk_bonus: 10, def_bonus: 5, hp_bonus: 0 },
];

pub struct TalentDef { pub id: u32, pub name: &'static str, pub class: u8, pub atk: i32, pub def: i32, pub hp: i32, pub icon: &'static str }
pub const TALENTS: &[TalentDef] = &[
    TalentDef { id: 1, name: "剑术精通", class: 1, atk: 5, def: 0, hp: 0, icon: "⚔" },
    TalentDef { id: 2, name: "铁壁",     class: 1, atk: 0, def: 5, hp: 0, icon: "🛡" },
    TalentDef { id: 3, name: "坚韧",     class: 1, atk: 0, def: 0, hp: 30, icon: "❤" },
    TalentDef { id: 4, name: "法术强化", class: 2, atk: 8, def: 0, hp: 0, icon: "💥" },
    TalentDef { id: 5, name: "魔力护盾", class: 2, atk: 0, def: 3, hp: 0, icon: "🔮" },
    TalentDef { id: 6, name: "法力潮汐", class: 2, atk: 0, def: 0, hp: 20, icon: "✨" },
    TalentDef { id: 7, name: "精准射击", class: 3, atk: 6, def: 0, hp: 0, icon: "🎯" },
    TalentDef { id: 8, name: "闪避步法", class: 3, atk: 0, def: 4, hp: 0, icon: "💨" },
    TalentDef { id: 9, name: "猎人直觉", class: 3, atk: 0, def: 0, hp: 25, icon: "👁" },
];

/// 怪物定义
pub struct MobDef {
    pub id: u32,
    pub name: &'static str,
    pub max_hp: i32,
    pub atk: i32,
    pub def: i32,
    pub exp: u32,
    pub level: u32,
    pub radius: f32,       // 巡逻半径
    pub detect_range: f32, // 仇恨范围
    pub attack_range: f32,
    pub attack_cd_ms: u64,
    pub move_speed: f32,
}

pub const MOB_DEFS: &[MobDef] = &[
    MobDef { id: 1, name: "史莱姆",     max_hp: 50,  atk: 8,  def: 2,  exp: 20,  level: 1, radius: 80.0,  detect_range: 120.0, attack_range: 30.0, attack_cd_ms: 2000, move_speed: 0.8 },
    MobDef { id: 2, name: "哥布林",     max_hp: 80,  atk: 12, def: 4,  exp: 35,  level: 2, radius: 100.0, detect_range: 150.0, attack_range: 35.0, attack_cd_ms: 1800, move_speed: 1.2 },
    MobDef { id: 3, name: "骷髅战士",   max_hp: 120, atk: 18, def: 8,  exp: 60,  level: 4, radius: 90.0,  detect_range: 140.0, attack_range: 35.0, attack_cd_ms: 1500, move_speed: 1.0 },
    MobDef { id: 4, name: "暗影法师",   max_hp: 90,  atk: 25, def: 3,  exp: 80,  level: 5, radius: 120.0, detect_range: 200.0, attack_range: 180.0, attack_cd_ms: 2200, move_speed: 0.6 },
    MobDef { id: 5, name: "岩石巨人",   max_hp: 300, atk: 30, def: 20, exp: 200, level: 8, radius: 60.0,  detect_range: 100.0, attack_range: 40.0, attack_cd_ms: 2500, move_speed: 0.5 },
    // v0.6: Boss 怪物 (地图专属, 高属性, 增强掉落)
    MobDef { id: 6, name: "森林守护者", max_hp: 1200, atk: 40, def: 25, exp: 500,  level: 12, radius: 100.0, detect_range: 200.0, attack_range: 80.0, attack_cd_ms: 1500, move_speed: 1.0 },
    MobDef { id: 7, name: "沙虫领主",   max_hp: 800,  atk: 55, def: 15, exp: 600,  level: 15, radius: 80.0,  detect_range: 250.0, attack_range: 60.0, attack_cd_ms: 1200, move_speed: 2.0 },
    MobDef { id: 8, name: "暗黑巫妖王", max_hp: 2000, atk: 65, def: 30, exp: 1000, level: 20, radius: 120.0, detect_range: 300.0, attack_range: 200.0, attack_cd_ms: 1000, move_speed: 0.8 },
];

/// NPC 定义
pub struct NpcDef {
    pub id: u32,
    pub name: &'static str,
    pub x: f32,
    pub y: f32,
    pub npc_type: &'static str, // "merchant", "quest_giver", "healer"
    pub dialog: &'static str,
}

pub const NPC_DEFS: &[NpcDef] = &[
    NpcDef { id: 1, name: "村长·李四",   x: 200.0, y: 200.0, npc_type: "quest_giver", dialog: "欢迎来到新手村！最近附近出现了不少怪物，能帮我们清理一些吗？" },
    NpcDef { id: 2, name: "商人·王五",   x: 1400.0, y: 200.0, npc_type: "merchant", dialog: "各种药水、装备应有尽有，来看看吧！" },
    NpcDef { id: 3, name: "治疗师·赵六", x: 800.0, y: 600.0, npc_type: "healer", dialog: "需要治疗吗？我可以免费为你恢复全部生命和法力。" },
    NpcDef { id: 4, name: "铁匠·孙七",   x: 1200.0, y: 800.0, npc_type: "merchant", dialog: "好剑配英雄！我可以帮你强化装备。" },
    NpcDef { id: 5, name: "公会会长",    x: 400.0, y: 1000.0, npc_type: "quest_giver", dialog: "想加入冒险者公会吗？先证明你的实力！" },
    // v0.6: 传送门 — 前往其他地图
    NpcDef { id: 6, name: "🌲 森林传送门", x: 1550.0, y: 600.0, npc_type: "portal", dialog: "前往幽暗森林... (地图2)" },
    NpcDef { id: 7, name: "🏜 沙漠传送门", x: 50.0, y: 600.0, npc_type: "portal", dialog: "前往烈日沙漠... (地图3)" },
    NpcDef { id: 8, name: "💀 地下城入口", x: 800.0, y: 1150.0, npc_type: "portal", dialog: "前往古老地下城... (地图4)" },
    // v0.6: 地图2(森林) 的返回传送门
    NpcDef { id: 9, name: "🏘 返回新手村", x: 50.0, y: 400.0, npc_type: "portal_map2", dialog: "返回新手村... (地图1)" },
    // v0.6: 地图3(沙漠) 的返回传送门
    NpcDef { id: 10, name: "🏘 返回新手村", x: 50.0, y: 400.0, npc_type: "portal_map3", dialog: "返回新手村... (地图1)" },
    // v0.6: 地图4(地下城) 的返回传送门
    NpcDef { id: 11, name: "🏘 返回新手村", x: 50.0, y: 400.0, npc_type: "portal_map4", dialog: "返回新手村... (地图1)" },
    // v0.6: Boss 副本入口 (交互后生成Boss)
    NpcDef { id: 12, name: "🌿 森林守护者祭坛", x: 1200.0, y: 400.0, npc_type: "dungeon", dialog: "触碰祭坛召唤森林守护者... (Boss Lv12)" },
    NpcDef { id: 13, name: "🏜 沙虫巢穴",       x: 1200.0, y: 400.0, npc_type: "dungeon2", dialog: "触碰巢穴召唤沙虫领主... (Boss Lv15)" },
    NpcDef { id: 14, name: "💀 巫妖王王座",     x: 1200.0, y: 400.0, npc_type: "dungeon3", dialog: "触碰王座召唤暗黑巫妖王... (Boss Lv20)" },
];

/// v0.6 地图定义
pub struct MapDef {
    pub id: u32,
    pub name: &'static str,
    pub bounds: (f32, f32, f32, f32), // (min_x, min_y, max_x, max_y)
    pub bg_color: &'static str,        // CSS color for client rendering
    pub mob_types: &'static [u32],     // 该地图生成的怪物类型
    pub portal_npc_ids: &'static [u32], // 该地图的传送门 NPC ID
}

pub const MAP_DEFS: &[MapDef] = &[
    MapDef { id: 1, name: "新手村",   bounds: (0.0, 0.0, 1600.0, 1200.0), bg_color: "#0d0d20",   mob_types: &[1, 2],       portal_npc_ids: &[6, 7, 8] },
    MapDef { id: 2, name: "幽暗森林", bounds: (0.0, 0.0, 1600.0, 1200.0), bg_color: "#0d1a0d",   mob_types: &[2, 3, 4],    portal_npc_ids: &[9] },
    MapDef { id: 3, name: "烈日沙漠", bounds: (0.0, 0.0, 1600.0, 1200.0), bg_color: "#1a1505",   mob_types: &[3, 4, 5],    portal_npc_ids: &[10] },
    MapDef { id: 4, name: "古老地下城", bounds: (0.0, 0.0, 1600.0, 1200.0), bg_color: "#0a0a0a", mob_types: &[4, 5],       portal_npc_ids: &[11] },
];

/// 物品定义
#[derive(Clone)]
pub struct ItemDef {
    pub id: u32,
    pub name: &'static str,
    pub item_type: &'static str, // "weapon", "armor", "accessory", "potion", "material"
    #[allow(dead_code)]
    pub value: u32,
    pub icon: &'static str,
    pub hp_restore: i32,
    pub mp_restore: i32,
    pub atk_bonus: i32,
    pub def_bonus: i32,
}

pub const ITEM_DEFS: &[ItemDef] = &[
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

// ── 商店数据 (v0.6) ──
#[derive(Debug, Clone)]
pub struct ShopItem {
    pub item_id: u32,
    pub price: u32,     // 金币购买价格
    pub sell_price: u32, // 卖回价格 (70%)
    pub stock: Option<u32>, // None = 无限存货
}

pub const SHOP_ITEMS: &[ShopItem] = &[
    ShopItem { item_id: 6,  price: 50,  sell_price: 35,  stock: None }, // 生命药水
    ShopItem { item_id: 7,  price: 50,  sell_price: 35,  stock: None }, // 法力药水
    ShopItem { item_id: 8,  price: 150, sell_price: 100, stock: Some(5) }, // 全恢复药水
    ShopItem { item_id: 1,  price: 100, sell_price: 70,  stock: None }, // 铁剑
    ShopItem { item_id: 2,  price: 300, sell_price: 200, stock: Some(3) }, // 钢剑
    ShopItem { item_id: 3,  price: 150, sell_price: 100, stock: None }, // 皮甲
    ShopItem { item_id: 4,  price: 400, sell_price: 280, stock: Some(3) }, // 铁甲
    ShopItem { item_id: 5,  price: 200, sell_price: 140, stock: Some(5) }, // 力量戒指
];

/// 任务定义
pub struct QuestDef {
    pub id: u32,
    pub name: &'static str,
    pub desc: &'static str,
    pub target_mob: u32,
    pub target_count: u32,
    pub exp_reward: u32,
    pub item_reward: u32, // item id
}

pub const QUEST_DEFS: &[QuestDef] = &[
    QuestDef { id: 1, name: "清除史莱姆",   desc: "消灭5只史莱姆",            target_mob: 1, target_count: 5,  exp_reward: 100,  item_reward: 6 },
    QuestDef { id: 2, name: "哥布林威胁",   desc: "消灭3只哥布林",            target_mob: 2, target_count: 3,  exp_reward: 200,  item_reward: 7 },
    QuestDef { id: 3, name: "骷髅清剿",     desc: "消灭2只骷髅战士",          target_mob: 3, target_count: 2,  exp_reward: 350,  item_reward: 1 },
    QuestDef { id: 4, name: "暗影威胁",     desc: "消灭1只暗影法师",          target_mob: 4, target_count: 1,  exp_reward: 500,  item_reward: 2 },
    QuestDef { id: 5, name: "巨人杀手",     desc: "消灭1只岩石巨人",          target_mob: 5, target_count: 1,  exp_reward: 1000, item_reward: 4 },
];

/// 升级所需经验
pub fn exp_for_level(level: u32) -> u32 {
    100 * level * level
}
