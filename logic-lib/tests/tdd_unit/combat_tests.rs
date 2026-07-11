//! 战斗服 TDD 单元测试 — 测试 src/combat 模块

use logic_lib::combat::damage::{DamageCalculator, CriticalHit};
use logic_lib::combat::{BattleState, BuffManager, BuffType, CombatManager, CombatStats, ExperienceCalculator};

#[test]
fn test_damage_calculation_formula() {
    // Base formula: max(1, atk * skill_mult - def * 0.5)
    let attacker = CombatStats { hp: 1000, max_hp: 1000, atk: 100, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 10, xp: 0, alive: true };
    let target = CombatStats { hp: 500, max_hp: 500, atk: 50, def: 40, crit_rate: 0.0, crit_dmg: 0.0, level: 5, xp: 0, alive: true };

    let mut calc = DamageCalculator::new();
    let (dmg, is_crit) = calc.calculate(&attacker, &target, 1.0);
    // raw = 100 * 1.0 = 100, def reduction = 40 * 0.5 = 20, dmg = 100 - 20 = 80
    assert_eq!(dmg, 80);
    assert!(!is_crit);

    // Test minimum damage (max of 1)
    let weak = CombatStats { hp: 100, max_hp: 100, atk: 1, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 1, xp: 0, alive: true };
    let (d2, _) = calc.calculate(&weak, &target, 1.0);
    assert_eq!(d2, 1, "最低伤害应为1");
}

#[test]
fn test_critical_hit_probability() {
    // 100% crit rate should always crit
    let attacker = CombatStats { hp: 1000, max_hp: 1000, atk: 100, def: 0, crit_rate: 100.0, crit_dmg: 2.0, level: 10, xp: 0, alive: true };
    let target = CombatStats { hp: 500, max_hp: 500, atk: 0, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 5, xp: 0, alive: true };

    let mut calc = DamageCalculator::new();
    let (dmg, is_crit) = calc.calculate(&attacker, &target, 1.0);
    assert!(is_crit, "100%暴击率必定暴击");
    assert_eq!(dmg, 200, "暴击伤害应为普通伤害的2倍: got {}", dmg);

    // 0% crit rate should never crit
    let no_crit = CombatStats { hp: 1000, max_hp: 1000, atk: 100, def: 0, crit_rate: 0.0, crit_dmg: 2.0, level: 10, xp: 0, alive: true };
    let (d2, is_crit2) = calc.calculate(&no_crit, &target, 1.0);
    assert!(!is_crit2, "0%暴击率不应暴击");
    assert_eq!(d2, 100);
}

#[test]
fn test_defense_damage_reduction() {
    // High defense reduces damage significantly
    let attacker = CombatStats { hp: 1000, max_hp: 1000, atk: 200, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 10, xp: 0, alive: true };
    let target = CombatStats { hp: 1000, max_hp: 1000, atk: 0, def: 200, crit_rate: 0.0, crit_dmg: 0.0, level: 10, xp: 0, alive: true };

    let mut calc = DamageCalculator::new();
    let (dmg, _) = calc.calculate(&attacker, &target, 1.0);
    // raw = 200, def_reduction = 200 * 0.5 = 100, dmg = 200 - 100 = 100
    assert_eq!(dmg, 100);

    // target with zero defense takes full damage
    let no_def = CombatStats { hp: 1000, max_hp: 1000, atk: 0, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 10, xp: 0, alive: true };
    let (d2, _) = calc.calculate(&attacker, &no_def, 1.0);
    assert_eq!(d2, 200, "无防御应承受全额伤害");
}

#[test]
fn test_damage_reduces_hp() {
    let mut mgr = CombatManager::new();
    let attacker = CombatStats { hp: 1000, max_hp: 1000, atk: 100, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 10, xp: 0, alive: true };
    let target = CombatStats { hp: 500, max_hp: 500, atk: 0, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 1, xp: 0, alive: true };

    mgr.create_entity(1, attacker);
    mgr.create_entity(2, target);

    let (dmg, _) = mgr.attack(1, 2, 1.0);
    assert_eq!(dmg, 100);
    assert_eq!(mgr.entity_stats(2).unwrap().hp, 400);
}

#[test]
fn test_hp_cannot_go_below_zero() {
    let mut mgr = CombatManager::new();
    let attacker = CombatStats { hp: 1000, max_hp: 1000, atk: 10000, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 99, xp: 0, alive: true };
    let target = CombatStats { hp: 100, max_hp: 100, atk: 0, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 1, xp: 0, alive: true };

    mgr.create_entity(1, attacker);
    mgr.create_entity(2, target);

    let (dmg, _) = mgr.attack(1, 2, 1.0);
    assert!(dmg > 100);
    assert_eq!(mgr.entity_stats(2).unwrap().hp, 0, "HP不能低于0");
}

#[test]
fn test_death_detection() {
    let mut mgr = CombatManager::new();
    let attacker = CombatStats { hp: 1000, max_hp: 1000, atk: 500, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 50, xp: 0, alive: true };
    let target = CombatStats { hp: 100, max_hp: 100, atk: 0, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 1, xp: 0, alive: true };

    mgr.create_entity(1, attacker);
    mgr.create_entity(2, target);

    assert!(!mgr.is_dead(2));
    mgr.attack(1, 2, 1.0);
    assert!(mgr.is_dead(2), "目标应死亡");
    assert!(!mgr.entity_stats(2).unwrap().alive);
}

#[test]
fn test_experience_gain() {
    let mut mgr = CombatManager::new();
    let attacker = CombatStats { hp: 1000, max_hp: 1000, atk: 500, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 10, xp: 0, alive: true };
    let target = CombatStats { hp: 100, max_hp: 100, atk: 0, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 10, xp: 0, alive: true };

    mgr.create_entity(1, attacker);
    mgr.create_entity(2, target);

    let calc = ExperienceCalculator::new();
    let xp = calc.calculate_xp_gain(10, 10); // killer_level=10, target_level=10
    assert!(xp > 0);

    // XP should be based on level difference
    let xp_high = calc.calculate_xp_gain(5, 10); // killer lower level vs higher target
    let xp_low = calc.calculate_xp_gain(50, 10); // killer much higher level
    assert!(xp_high > xp_low, "击杀等级更高的目标应获得更多经验");
}

#[test]
fn test_level_up() {
    let attacker = CombatStats { hp: 1000, max_hp: 1000, atk: 100, def: 50, crit_rate: 5.0, crit_dmg: 1.5, level: 5, xp: 450, alive: true };
    let calc = ExperienceCalculator::new();
    let new_lvl = calc.check_level_up(attacker.xp, attacker.level);
    assert!(!new_lvl, "经验不足不应升级");

    let ready = CombatStats { hp: 1000, max_hp: 1000, atk: 100, def: 50, crit_rate: 5.0, crit_dmg: 1.5, level: 5, xp: 550, alive: true };
    let leveled = calc.check_level_up(ready.xp, ready.level);
    assert!(leveled, "经验足够应升级");
}

#[test]
fn test_buff_application() {
    let mut mgr = CombatManager::new();
    let mut buf_mgr = BuffManager::new();

    let entity = CombatStats { hp: 100, max_hp: 100, atk: 50, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 1, xp: 0, alive: true };
    mgr.create_entity(1, entity);

    // Apply attack buff
    buf_mgr.apply(1, BuffType::AttackUp { value: 30 }, 10);
    let active = buf_mgr.active_buffs(1);
    assert_eq!(active.len(), 1);

    let atk_mod = buf_mgr.get_atk_modifier(1);
    assert_eq!(atk_mod, 30);

    // Apply defense debuff
    buf_mgr.apply(1, BuffType::DefenseDown { value: 20 }, 10);
    let def_mod = buf_mgr.get_def_modifier(1);
    assert_eq!(def_mod, -20);
}

#[test]
fn test_buff_removal() {
    let mut buf_mgr = BuffManager::new();
    buf_mgr.apply(1, BuffType::AttackUp { value: 50 }, 0); // duration 0
    buf_mgr.tick(1); // tick should remove expired

    let active = buf_mgr.active_buffs(1);
    assert_eq!(active.len(), 0, "过期Buff应被移除");
    assert_eq!(buf_mgr.get_atk_modifier(1), 0);
}

#[test]
fn test_aoe_damage() {
    let mut mgr = CombatManager::new();
    let attacker = CombatStats { hp: 1000, max_hp: 1000, atk: 200, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 15, xp: 0, alive: true };

    mgr.create_entity(1, attacker);
    for i in 2..=4 {
        let t = CombatStats { hp: 300, max_hp: 300, atk: 0, def: 20, crit_rate: 0.0, crit_dmg: 0.0, level: 5, xp: 0, alive: true };
        mgr.create_entity(i, t);
    }

    let results = mgr.aoe_attack(1, &[2, 3, 4], 0.8);
    assert_eq!(results.len(), 3);
    for (tid, dmg) in &results {
        assert!(*dmg > 0, "目标 {} 应受伤害", tid);
        assert_eq!(mgr.entity_stats(*tid).unwrap().hp, 300 - dmg);
    }
}

#[test]
fn test_combat_state_machine() {
    let mut mgr = CombatManager::new();
    let entity = CombatStats { hp: 100, max_hp: 100, atk: 50, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 1, xp: 0, alive: true };
    mgr.create_entity(1, entity);

    assert_eq!(mgr.battle_state(1), BattleState::Idle);

    mgr.enter_combat(1);
    assert_eq!(mgr.battle_state(1), BattleState::Fighting);

    mgr.exit_combat(1);
    assert_eq!(mgr.battle_state(1), BattleState::Idle);

    // Dead entity cannot enter combat
    let dead = CombatStats { hp: 0, max_hp: 100, atk: 0, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 1, xp: 0, alive: false };
    mgr.create_entity(2, dead);
    mgr.enter_combat(2);
    assert_eq!(mgr.battle_state(2), BattleState::Idle, "死亡实体不能进入战斗");
}

/// CriticalHit struct tests
#[test]
fn test_critical_hit_struct() {
    let crit = CriticalHit { chance: 50.0, multiplier: 2.5 };
    assert_eq!(crit.chance, 50.0);
    assert_eq!(crit.multiplier, 2.5);
}

#[test]
fn test_experience_calculator() {
    let calc = ExperienceCalculator::new();
    // Same level
    let xp = calc.calculate_xp_gain(10, 10);
    assert!(xp > 0);

    // Level diff: killer lower level = bonus XP
    let bonus = calc.calculate_xp_gain(5, 10);
    assert!(bonus > xp);

    // Level diff: killer much higher = reduced XP (but min 1)
    let min_xp = calc.calculate_xp_gain(100, 1);
    assert_eq!(min_xp, 1, "击杀远低于自己等级的目标经验最低为1");
}

#[test]
fn test_attack_dead_target() {
    let mut mgr = CombatManager::new();
    let attacker = CombatStats { hp: 100, max_hp: 100, atk: 10, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 1, xp: 0, alive: true };
    let dead = CombatStats { hp: 0, max_hp: 100, atk: 0, def: 0, crit_rate: 0.0, crit_dmg: 0.0, level: 1, xp: 0, alive: false };

    mgr.create_entity(1, attacker);
    mgr.create_entity(2, dead);

    let (dmg, _) = mgr.attack(1, 2, 1.0);
    assert_eq!(dmg, 0, "攻击已死亡的实体伤害应为0");
}

#[test]
fn test_multiple_buffs_stack() {
    let mut buf_mgr = BuffManager::new();
    buf_mgr.apply(1, BuffType::AttackUp { value: 30 }, 10);
    buf_mgr.apply(1, BuffType::AttackUp { value: 20 }, 10);
    assert_eq!(buf_mgr.get_atk_modifier(1), 50); // Stacking

    buf_mgr.apply(1, BuffType::DefenseDown { value: 15 }, 10);
    buf_mgr.apply(1, BuffType::DefenseDown { value: 25 }, 10);
    assert_eq!(buf_mgr.get_def_modifier(1), -40);
}
