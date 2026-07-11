//! 战斗管理器 — 攻击计算、伤害结算、Buff管理、经验系统
//!
//! 提供核心战斗逻辑供 gRPC 战斗服使用。

pub mod damage;

use std::collections::HashMap;

/// 实体战斗属性
#[derive(Debug, Clone)]
pub struct CombatStats {
    pub hp: i64,
    pub max_hp: i64,
    pub atk: i64,
    pub def: i64,
    pub crit_rate: f64,    // 百分比 0-100
    pub crit_dmg: f64,     // 倍率
    pub level: u32,
    pub xp: u64,
    pub alive: bool,
}

/// 战斗状态
#[derive(Debug, Clone, PartialEq)]
pub enum BattleState {
    Idle,
    Fighting,
}

/// Buff 类型
#[derive(Debug, Clone)]
pub enum BuffType {
    AttackUp { value: i64 },
    DefenseDown { value: i64 },
}

/// 单个 Buff 实例
#[derive(Debug, Clone)]
pub struct BuffInstance {
    pub buff_type: BuffType,
    pub remaining_ticks: u32,
}

// ════════════════════════════════════════════
// 伤害计算器
// ════════════════════════════════════════════

pub use damage::{CriticalHit, DamageCalculator};

// ════════════════════════════════════════════
// 经验计算器
// ════════════════════════════════════════════

pub struct ExperienceCalculator;

impl ExperienceCalculator {
    pub fn new() -> Self {
        Self
    }

    /// 根据击杀者等级和目标等级计算经验值
    pub fn calculate_xp_gain(&self, killer_level: u32, target_level: u32) -> u64 {
        let base_xp = target_level as u64 * 50;
        let level_diff = target_level as i64 - killer_level as i64;

        if level_diff > 0 {
            // 击杀高级目标获得额外经验
            (base_xp as f64 * (1.0 + level_diff as f64 * 0.2)) as u64
        } else if level_diff < -20 {
            // 击杀远低于自己等级的目标经验最低为1
            1
        } else {
            // 同级或略低则正常
            let reduction = (-level_diff) as f64 * 0.05;
            ((base_xp as f64 * (1.0 - reduction)) as u64).max(1)
        }
    }

    /// 检查是否升级 (当前经验 >= 升级所需经验)
    pub fn check_level_up(&self, xp: u64, level: u32) -> bool {
        let xp_needed: u64 = level as u64 * 100;
        xp >= xp_needed
    }

    /// 获取升级所需经验
    pub fn xp_for_level(&self, level: u32) -> u64 {
        level as u64 * 100
    }
}

// ════════════════════════════════════════════
// Buff 管理器
// ════════════════════════════════════════════

pub struct BuffManager {
    buffs: HashMap<u64, Vec<BuffInstance>>,
}

impl BuffManager {
    pub fn new() -> Self {
        Self { buffs: HashMap::new() }
    }

    /// 给实体添加 Buff
    pub fn apply(&mut self, entity_id: u64, buff_type: BuffType, duration_ticks: u32) {
        self.buffs
            .entry(entity_id)
            .or_default()
            .push(BuffInstance { buff_type, remaining_ticks: duration_ticks });
    }

    /// 删除实体所有 Buff
    pub fn remove_all(&mut self, entity_id: u64) {
        self.buffs.remove(&entity_id);
    }

    /// 获取实体的活跃 Buff 列表
    pub fn active_buffs(&self, entity_id: u64) -> Vec<&BuffInstance> {
        self.buffs.get(&entity_id).map(|v| v.iter().collect()).unwrap_or_default()
    }

    /// 时间流逝，减少 Buff 剩余回合
    pub fn tick(&mut self, entity_id: u64) {
        if let Some(buffs) = self.buffs.get_mut(&entity_id) {
            buffs.retain(|b| b.remaining_ticks > 0);
            for b in buffs.iter_mut() {
                if b.remaining_ticks > 0 {
                    b.remaining_ticks -= 1;
                }
            }
            // 移除过期 Buff
            buffs.retain(|b| b.remaining_ticks > 0 || matches!(b.buff_type, BuffType::AttackUp { .. } | BuffType::DefenseDown { .. }));
            // 再次清理已过期的
            buffs.retain(|b| b.remaining_ticks > 0);
            if buffs.is_empty() {
                self.buffs.remove(&entity_id);
            }
        }
    }

    /// 获取攻击力修正值
    pub fn get_atk_modifier(&self, entity_id: u64) -> i64 {
        self.buffs
            .get(&entity_id)
            .map(|v| {
                v.iter()
                    .filter(|b| matches!(b.buff_type, BuffType::AttackUp { .. }))
                    .map(|b| if let BuffType::AttackUp { value } = b.buff_type { value } else { 0 })
                    .sum()
            })
            .unwrap_or(0)
    }

    /// 获取防御力修正值 (负数表示降低)
    pub fn get_def_modifier(&self, entity_id: u64) -> i64 {
        self.buffs
            .get(&entity_id)
            .map(|v| {
                v.iter()
                    .filter(|b| matches!(b.buff_type, BuffType::DefenseDown { .. }))
                    .map(|b| if let BuffType::DefenseDown { value } = b.buff_type { -(value as i64) } else { 0 })
                    .sum()
            })
            .unwrap_or(0)
    }

    /// 对所有实体 tick
    pub fn tick_all(&mut self) {
        let ids: Vec<u64> = self.buffs.keys().cloned().collect();
        for id in ids {
            self.tick(id);
        }
    }
}

// ════════════════════════════════════════════
// 战斗管理器
// ════════════════════════════════════════════

pub struct CombatManager {
    entities: HashMap<u64, CombatStats>,
    states: HashMap<u64, BattleState>,
    damage_calc: DamageCalculator,
    pub buff_manager: BuffManager,
    pub xp_calc: ExperienceCalculator,
}

impl CombatManager {
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            states: HashMap::new(),
            damage_calc: DamageCalculator::new(),
            buff_manager: BuffManager::new(),
            xp_calc: ExperienceCalculator,
        }
    }

    /// 创建战斗实体
    pub fn create_entity(&mut self, id: u64, stats: CombatStats) {
        self.entities.insert(id, stats);
        self.states.insert(id, BattleState::Idle);
    }

    /// 获取实体属性
    pub fn entity_stats(&self, id: u64) -> Option<&CombatStats> {
        self.entities.get(&id)
    }

    /// 获取实体可变属性
    pub fn entity_stats_mut(&mut self, id: u64) -> Option<&mut CombatStats> {
        self.entities.get_mut(&id)
    }

    /// 检查实体是否死亡
    pub fn is_dead(&self, id: u64) -> bool {
        self.entities.get(&id).map(|e| !e.alive).unwrap_or(true)
    }

    /// 获取战斗状态
    pub fn battle_state(&self, id: u64) -> BattleState {
        self.states.get(&id).cloned().unwrap_or(BattleState::Idle)
    }

    /// 进入战斗状态
    pub fn enter_combat(&mut self, id: u64) -> bool {
        if self.is_dead(id) {
            return false;
        }
        self.states.insert(id, BattleState::Fighting);
        true
    }

    /// 退出战斗状态
    pub fn exit_combat(&mut self, id: u64) {
        self.states.insert(id, BattleState::Idle);
    }

    /// 攻击目标，返回 (伤害值, 是否暴击)
    pub fn attack(&mut self, attacker_id: u64, target_id: u64, skill_mult: f64) -> (i64, bool) {
        // 检查攻击者和目标是否存在
        if !self.entities.contains_key(&attacker_id) || !self.entities.contains_key(&target_id) {
            return (0, false);
        }

        // 如果目标已死亡，不造成额外伤害
        if self.is_dead(target_id) {
            return (0, false);
        }

        // 获取攻击者基础属性并应用 Buff 修正
        let mut attacker_stats = self.entities[&attacker_id].clone();
        attacker_stats.atk += self.buff_manager.get_atk_modifier(attacker_id);
        attacker_stats.def += self.buff_manager.get_def_modifier(attacker_id);

        // 获取目标基础属性并应用 Buff 修正
        let mut target_stats = self.entities[&target_id].clone();
        target_stats.atk += self.buff_manager.get_atk_modifier(target_id);
        target_stats.def += self.buff_manager.get_def_modifier(target_id);

        let (dmg, is_crit) = self.damage_calc.calculate(&attacker_stats, &target_stats, skill_mult);

        // 应用伤害
        if let Some(target) = self.entities.get_mut(&target_id) {
            target.hp = (target.hp - dmg).max(0);
            if target.hp == 0 {
                target.alive = false;
                self.states.insert(target_id, BattleState::Idle);

                // 经验计算
                let xp = self.xp_calc.calculate_xp_gain(attacker_stats.level, target_stats.level);
                if let Some(attacker) = self.entities.get_mut(&attacker_id) {
                    attacker.xp += xp;
                    // 检查升级
                    if self.xp_calc.check_level_up(attacker.xp, attacker.level) {
                        attacker.level += 1;
                    }
                }
            }
        }

        // 攻击者进入战斗状态
        self.states.insert(attacker_id, BattleState::Fighting);

        (dmg, is_crit)
    }

    /// AOE 攻击，返回每个目标的伤害
    pub fn aoe_attack(&mut self, attacker_id: u64, target_ids: &[u64], skill_mult: f64) -> Vec<(u64, i64)> {
        let mut results = Vec::new();
        for &tid in target_ids {
            let (dmg, _) = self.attack(attacker_id, tid, skill_mult);
            results.push((tid, dmg));
        }
        results
    }

    /// 移除实体
    pub fn remove_entity(&mut self, id: u64) {
        self.entities.remove(&id);
        self.states.remove(&id);
        self.buff_manager.remove_all(id);
    }

    /// 获取所有实体 ID
    pub fn entity_ids(&self) -> Vec<u64> {
        self.entities.keys().cloned().collect()
    }

    /// 实体总数
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }
}
