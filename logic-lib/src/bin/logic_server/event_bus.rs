// ════════════════════════════════════════════════════════════════
// 事件总线 — 解耦业务模块
// 杀怪/拾取/完成任务等事件通过总线分发，订阅者互不直接调用
// ════════════════════════════════════════════════════════════════

use super::utils::*;
use rust_mmo_gate::grpc_router::proto::gate::DownstreamMessage;

/// 游戏事件
#[derive(Debug, Clone)]
pub enum GameEvent {
    /// 怪物被击杀
    MobKilled {
        killer_uid: u64,
        mob_def_id: u32,
        mob_entity_id: u64,
        mob_name: String,
        x: f32,
        y: f32,
    },
    /// 物品被拾取
    #[allow(dead_code)]
    ItemPicked {
        picker_uid: u64,
        item_id: u32,
        count: u32,
    },
    /// 任务进度更新
    #[allow(dead_code)]
    QuestProgressed {
        player_uid: u64,
        quest_id: u32,
        target_mob: u32,
        new_progress: u32,
        target_count: u32,
    },
    /// 任务完成
    #[allow(dead_code)]
    QuestCompleted {
        player_uid: u64,
        quest_id: u32,
    },
    /// 玩家升级
    #[allow(dead_code)]
    PlayerLeveledUp {
        uid: u64,
        new_level: u32,
    },
}

/// 事件处理的副作用
#[derive(Debug, Default)]
pub struct SideEffect {
    /// 要发给玩家的消息 (uid, message)
    pub player_messages: Vec<(u64, DownstreamMessage)>,
    /// 要广播的消息
    pub broadcast_messages: Vec<DownstreamMessage>,
    /// 经验奖励 (uid, exp)
    pub exp_rewards: Vec<(u64, u32)>,
    /// 物品奖励 (uid, item_id, count)
    #[allow(dead_code)]
    pub item_rewards: Vec<(u64, u32, u32)>,
}

impl SideEffect {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_player_msg(&mut self, uid: u64, msg: DownstreamMessage) {
        self.player_messages.push((uid, msg));
    }

    pub fn add_broadcast(&mut self, msg: DownstreamMessage) {
        self.broadcast_messages.push(msg);
    }

    pub fn add_exp(&mut self, uid: u64, exp: u32) {
        self.exp_rewards.push((uid, exp));
    }

    #[allow(dead_code)]
    pub fn add_item(&mut self, uid: u64, item_id: u32, count: u32) {
        self.item_rewards.push((uid, item_id, count));
    }

    /// 合并另一个 SideEffect
    pub fn merge(&mut self, other: SideEffect) {
        self.player_messages.extend(other.player_messages);
        self.broadcast_messages.extend(other.broadcast_messages);
        self.exp_rewards.extend(other.exp_rewards);
        self.item_rewards.extend(other.item_rewards);
    }
}

/// 事件订阅者 trait
pub trait EventHandler: Send + Sync {
    fn handle(&self, event: &GameEvent, state: &super::state::GameState) -> SideEffect;
}

/// 事件总线
pub struct EventBus {
    handlers: Vec<Box<dyn EventHandler>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// 创建带默认订阅者的事件总线
    pub fn with_default_handlers() -> Self {
        let mut bus = Self::new();
        bus.subscribe(Box::new(QuestProgressHandler));
        bus.subscribe(Box::new(DropGenerationHandler));
        bus.subscribe(Box::new(ExperienceRewardHandler));
        bus
    }

    pub fn subscribe(&mut self, handler: Box<dyn EventHandler>) {
        self.handlers.push(handler);
    }

    /// 发布事件，收集所有订阅者的副作用
    pub fn publish(&self, event: &GameEvent, state: &super::state::GameState) -> SideEffect {
        let mut effect = SideEffect::new();
        for handler in &self.handlers {
            let h_effect = handler.handle(event, state);
            effect.merge(h_effect);
        }
        effect
    }
}

// ════════════════════════════════════════════════════════════════
// 默认订阅者实现
// ════════════════════════════════════════════════════════════════

/// 任务进度订阅者：监听 MobKilled，更新任务进度
pub struct QuestProgressHandler;

impl EventHandler for QuestProgressHandler {
    fn handle(&self, event: &GameEvent, state: &super::state::GameState) -> SideEffect {
        let mut effect = SideEffect::new();
        if let GameEvent::MobKilled {
            killer_uid,
            mob_def_id,
            ..
        } = event
        {
            if let Some(mut player) = state.players.get_mut(killer_uid) {
                let updated = player.update_quest_progress(*mob_def_id);
                if updated {
                    effect.add_player_msg(
                        *killer_uid,
                        super::codec::dm_proto(*killer_uid, 5005, &player.to_quests_proto(), 1),
                    );
                }
            }
        }
        effect
    }
}

/// 掉落生成订阅者：监听 MobKilled，生成掉落物并广播死亡信息
pub struct DropGenerationHandler;

impl EventHandler for DropGenerationHandler {
    fn handle(&self, event: &GameEvent, state: &super::state::GameState) -> SideEffect {
        let mut effect = SideEffect::new();
        if let GameEvent::MobKilled {
            killer_uid,
            mob_def_id,
            mob_entity_id,
            mob_name,
            x,
            y,
        } = event
        {
            let drops = state.generate_drops(*mob_def_id, *x, *y);
            let drop_json: Vec<String> = drops.iter().map(|d| d.to_json()).collect();

            // 插入掉落物到世界
            for drop in &drops {
                state.drops.insert(drop.drop_id, drop.clone());
            }

            let mob_exp = get_mob_def(*mob_def_id).map(|d| d.exp).unwrap_or(0);
            let death_json = serde_json::json!({
                "entityId": mob_entity_id,
                "killer": killer_uid,
                "killerName": format!("Player{}", killer_uid),
                "mobName": mob_name,
                "drops": drop_json,
                "exp": mob_exp,
            })
            .to_string();
            effect.add_broadcast(dm(0, 6003, death_json, 1));
        }
        effect
    }
}

/// 经验奖励订阅者：监听 MobKilled，给击杀者经验
pub struct ExperienceRewardHandler;

impl EventHandler for ExperienceRewardHandler {
    fn handle(&self, event: &GameEvent, _state: &super::state::GameState) -> SideEffect {
        let mut effect = SideEffect::new();
        if let GameEvent::MobKilled {
            killer_uid,
            mob_def_id,
            ..
        } = event
        {
            if let Some(def) = get_mob_def(*mob_def_id) {
                effect.add_exp(*killer_uid, def.exp);
            }
        }
        effect
    }
}
