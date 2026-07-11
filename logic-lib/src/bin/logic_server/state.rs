// ════════════════════════════════════════════════════════════════
// 逻辑服实现 — 状态结构
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use super::types::*;
use super::utils::*;
use dashmap::DashMap;
use std::sync::Arc;

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
    // v0.6: 公会系统
    pub guilds: DashMap<String, GuildInfo>,
    pub player_guild: DashMap<u64, String>, // uid → guild_name
    // v0.6: PvP 决斗
    pub duel_requests: DashMap<u64, u64>, // challenger_uid → target_uid
}

pub struct MockLogicService {
    pub state: Arc<GameState>,
    pub db: Option<Arc<logic_lib::db::Database>>,
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
            guilds: DashMap::new(),
            player_guild: DashMap::new(),
            duel_requests: DashMap::new(),
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

        tracing::info!(npcs = state.npcs.len(), mobs = state.mobs.len(), "已生成 NPC 与怪物");

        let state = Arc::new(state);

        // ====== 后台游戏循环：独立 OS 线程驱动怪物 AI，避免阻塞 tokio runtime ======
        let bg = state.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                bg.tick_mob_ai(0);
                // 同步更新 last_mob_tick，让 4002 handler 节流生效
                bg.last_mob_tick.store(current_millis(), std::sync::atomic::Ordering::Relaxed);
            }
        });

        MockLogicService { state, db: None }
    }
}

impl GameState {
    /// 测试用构造方法：不 spawn 后台 AI 循环任务
    pub fn test_new() -> GameState {
        GameState {
            players: DashMap::new(),
            mobs: DashMap::new(),
            npcs: NPC_DEFS.iter().map(NpcEntity::from_def).collect(),
            drops: DashMap::new(),
            next_entity_id: std::sync::atomic::AtomicU64::new(10000),
            next_drop_id: std::sync::atomic::AtomicU64::new(20000),
            online_count: std::sync::atomic::AtomicU64::new(0),
            last_mob_tick: std::sync::atomic::AtomicU64::new(0),
            party_mgr: logic_lib::party::PartyManager::new(),
            guilds: DashMap::new(),
            player_guild: DashMap::new(),
            duel_requests: DashMap::new(),
        }
    }
}
