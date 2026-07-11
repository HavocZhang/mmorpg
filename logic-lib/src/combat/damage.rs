//! 伤害计算器 — 基础公式、暴击判定、防御减伤

use super::CombatStats;
use rand::Rng;
use rand::SeedableRng;

/// 暴击配置
#[derive(Debug, Clone, PartialEq)]
pub struct CriticalHit {
    pub chance: f64,
    pub multiplier: f64,
}

/// 伤害计算器
pub struct DamageCalculator {
    rng: rand::rngs::StdRng,
}

impl DamageCalculator {
    pub fn new() -> Self {
        Self {
            rng: rand::rngs::StdRng::from_entropy(),
        }
    }

    /// 创建指定种子的计算器（用于确定性测试）
    #[allow(dead_code)]
    pub fn with_seed(seed: u64) -> Self {
        use rand::SeedableRng;
        Self {
            rng: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }

    /// 计算伤害
    ///
    /// 公式: max(1, floor(atk * skill_mult - def * 0.5) * crit_mult)
    /// 其中暴击概率取自 attacker.crit_rate (百分比)，暴击倍率取自 attacker.crit_dmg
    pub fn calculate(&mut self, attacker: &CombatStats, target: &CombatStats, skill_mult: f64) -> (i64, bool) {
        let raw_damage = (attacker.atk as f64 * skill_mult) as i64;
        let def_reduction = (target.def as f64 * 0.5) as i64;
        let base_damage = (raw_damage - def_reduction).max(1);

        // 暴击判定
        let roll: f64 = self.rng.gen_range(0.0..100.0);
        let is_crit = roll < attacker.crit_rate;

        let final_damage = if is_crit {
            (base_damage as f64 * attacker.crit_dmg) as i64
        } else {
            base_damage
        };

        (final_damage.max(1), is_crit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stats(atk: i64, def: i64, crit_rate: f64, crit_dmg: f64, level: u32) -> CombatStats {
        CombatStats { hp: 1000, max_hp: 1000, atk, def, crit_rate, crit_dmg, level, xp: 0, alive: true }
    }

    #[test]
    fn test_basic_damage() {
        let mut calc = DamageCalculator::with_seed(42);
        let a = make_stats(100, 0, 0.0, 0.0, 10);
        let d = make_stats(0, 0, 0.0, 0.0, 1);
        let (dmg, is_crit) = calc.calculate(&a, &d, 1.0);
        assert_eq!(dmg, 100);
        assert!(!is_crit);
    }

    #[test]
    fn test_defense_reduction() {
        let mut calc = DamageCalculator::with_seed(42);
        let a = make_stats(200, 0, 0.0, 0.0, 10);
        let d = make_stats(0, 100, 0.0, 0.0, 10);
        let (dmg, _) = calc.calculate(&a, &d, 1.0);
        assert_eq!(dmg, 150); // 200 - 100*0.5 = 150
    }

    #[test]
    fn test_minimum_damage() {
        let mut calc = DamageCalculator::with_seed(42);
        let a = make_stats(1, 0, 0.0, 0.0, 1);
        let d = make_stats(0, 0, 0.0, 0.0, 1);
        let (dmg, _) = calc.calculate(&a, &d, 1.0);
        assert_eq!(dmg, 1);
    }

    #[test]
    fn test_guaranteed_crit() {
        let mut calc = DamageCalculator::new();
        let a = make_stats(100, 0, 100.0, 2.0, 10);
        let d = make_stats(0, 0, 0.0, 0.0, 1);
        let (dmg, is_crit) = calc.calculate(&a, &d, 1.0);
        assert!(is_crit);
        assert_eq!(dmg, 200); // 100 * 2.0
    }

    #[test]
    fn test_skill_multiplier() {
        let mut calc = DamageCalculator::with_seed(42);
        let a = make_stats(100, 0, 0.0, 0.0, 10);
        let d = make_stats(0, 0, 0.0, 0.0, 1);
        let (dmg, _) = calc.calculate(&a, &d, 2.5);
        assert_eq!(dmg, 250); // 100 * 2.5
    }
}
