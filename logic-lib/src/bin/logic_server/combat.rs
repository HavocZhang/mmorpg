// ════════════════════════════════════════════════════════════════
// 逻辑服实现 — 战斗处理 (impl GameState)
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use super::event_bus;
use super::state::*;
use super::types::*;
use super::utils::*;
use logic_lib::game_proto as gp;
use rust_mmo_gate::grpc_router::proto::gate::*;

impl GameState {
    // ════════════════════════════════════════════════════════════
    // 战斗处理
    // ════════════════════════════════════════════════════════════
    pub fn handle_attack(&self, uid: u64, skill_id: u32, target_uid: u64) -> Vec<DownstreamMessage> {
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

        // 更新MP (proto: ExpUpdate with is_mp_update=true)
        let (mp_now, max_mp_now) = self
            .players
            .get(&uid)
            .map(|p| (p.mp, p.max_mp))
            .unwrap_or((0, 50));
        let mp_update = gp::ExpUpdate {
            exp: 0,
            max_exp: 0,
            level: 0,
            gained: 0,
            mp: mp_now,
            max_mp: max_mp_now,
            is_mp_update: true,
        };
        messages.push(super::codec::dm_proto(uid, 5002, &mp_update, 1));

        // 目标是怪物实体 (先查 mobs 表)
        if target_uid >= 10000 && self.mobs.contains_key(&target_uid) {
            let mut mob = match self.mobs.get_mut(&target_uid) {
                Some(m) => m,
                None => {
                    let miss = gp::CombatResult {
                        target_uid,
                        damage: 0,
                        target_hp: 0,
                        crit: false,
                        miss: true,
                        player_hp: 0,
                        exp_gained: 0,
                        error: String::new(),
                        cooldown_left: 0,
                        target_name: String::new(),
                        skill_id,
                        reason: String::new(),
                        swing: false,
                        attacker_uid: uid,
                    };
                    messages.push(super::codec::dm_proto(uid, 6001, &miss, 2));
                    return messages;
                }
            };

            if mob.state == MobState::Dead {
                let miss = gp::CombatResult {
                    target_uid,
                    damage: 0,
                    target_hp: 0,
                    crit: false,
                    miss: true,
                    player_hp: 0,
                    exp_gained: 0,
                    error: String::new(),
                    cooldown_left: 0,
                    target_name: mob.name.clone(),
                    skill_id,
                    reason: String::new(),
                    swing: false,
                    attacker_uid: uid,
                };
                messages.push(super::codec::dm_proto(uid, 6001, &miss, 2));
                return messages;
            }

            // 检查距离
            let dist = distance(player_x, player_y, mob.x, mob.y);
            if dist > skill.range + 20.0 {
                let miss = gp::CombatResult {
                    target_uid,
                    damage: 0,
                    target_hp: mob.hp,
                    crit: false,
                    miss: true,
                    player_hp: 0,
                    exp_gained: 0,
                    error: String::new(),
                    cooldown_left: 0,
                    target_name: mob.name.clone(),
                    skill_id,
                    reason: "out_of_range".to_string(),
                    swing: false,
                    attacker_uid: uid,
                };
                messages.push(super::codec::dm_proto(uid, 6001, &miss, 2));
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
            let mob_name = mob.name.clone();

            // 给攻击者发战斗结果 (proto)
            let battle = gp::CombatResult {
                target_uid,
                damage: final_dmg,
                target_hp: mob_hp,
                crit,
                miss: false,
                player_hp: 0,
                exp_gained: 0,
                error: String::new(),
                cooldown_left: 0,
                target_name: mob_name.clone(),
                skill_id,
                reason: String::new(),
                swing: false,
                attacker_uid: uid,
            };
            messages.push(super::codec::dm_proto(uid, 6001, &battle, 2));

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
                mob.last_attack = now; // reuse as death time
            }
            drop(mob);

            if mob_hp == 0 {
                tracing::info!(uid, target_uid, "mob killed");
                // 通过事件总线分发杀怪事件，解耦任务进度/掉落/经验
                let event = event_bus::GameEvent::MobKilled {
                    killer_uid: uid,
                    mob_def_id,
                    mob_entity_id: target_uid,
                    mob_name: mob_name.clone(),
                    x: mob_x,
                    y: mob_y,
                };
                let effect = self.event_bus.publish(&event, self);

                // 应用副作用：经验奖励（含升级、金币）
                for (target_uid_exp, exp) in &effect.exp_rewards {
                    if let Some(mut p) = self.players.get_mut(target_uid_exp) {
                        let leveled_up = p.add_exp(*exp);
                        // v0.6: 击杀奖励金币 = 怪物等级 * 5
                        let gold_reward = (get_mob_def(mob_def_id).map(|d| d.level).unwrap_or(1) * 5) as u32;
                        p.gold += gold_reward;

                        // 5002 经验更新 (proto: ExpUpdate with is_mp_update=false)
                        let exp_update = gp::ExpUpdate {
                            exp: p.exp,
                            max_exp: exp_for_level(p.level),
                            level: p.level,
                            gained: *exp,
                            mp: 0,
                            max_mp: 0,
                            is_mp_update: false,
                        };
                        messages.push(super::codec::dm_proto(*target_uid_exp, 5002, &exp_update, 1));

                        if leveled_up {
                            // 升级消息保持 JSON：只发部分字段（与完整 PlayerStats 字段集不一致）
                            let levelup_json = serde_json::json!({
                                "level": p.level,
                                "maxHp": p.max_hp,
                                "maxMp": p.max_mp,
                                "hp": p.hp,
                                "mp": p.mp,
                                "atk": p.total_atk(),
                                "def": p.total_def(),
                            }).to_string();
                            messages.push(dm(*target_uid_exp, 5001, levelup_json, 2));

                            let broadcast = serde_json::json!({
                                "from": 0,
                                "fromName": "System",
                                "text": format!("Player{} 升到了 {} 级!", target_uid_exp, p.level),
                            }).to_string();
                            messages.push(dm(0, 7002, broadcast, 1));
                        }
                    }
                }

                // 应用副作用：玩家定向消息（如任务进度更新 5005）
                for (_, msg) in effect.player_messages {
                    messages.push(msg);
                }

                // 应用副作用：广播消息（如死亡掉落 6003）
                for msg in effect.broadcast_messages {
                    messages.push(msg);
                }
            }

            return messages;
        }

        // 目标是玩家 (查 players 表, 不限 UID 范围)
        if target_uid > 0 && self.players.contains_key(&target_uid) {
            let mut target = match self.players.get_mut(&target_uid) {
                Some(t) => t,
                None => {
                    let miss = gp::CombatResult {
                        target_uid,
                        damage: 0,
                        target_hp: 0,
                        crit: false,
                        miss: true,
                        player_hp: 0,
                        exp_gained: 0,
                        error: String::new(),
                        cooldown_left: 0,
                        target_name: String::new(),
                        skill_id,
                        reason: String::new(),
                        swing: false,
                        attacker_uid: uid,
                    };
                    messages.push(super::codec::dm_proto(uid, 6001, &miss, 2));
                    return messages;
                }
            };

            let dist = distance(player_x, player_y, target.x, target.y);
            if dist > skill.range + 20.0 {
                let miss = gp::CombatResult {
                    target_uid,
                    damage: 0,
                    target_hp: target.hp,
                    crit: false,
                    miss: true,
                    player_hp: 0,
                    exp_gained: 0,
                    error: String::new(),
                    cooldown_left: 0,
                    target_name: target.name.clone(),
                    skill_id,
                    reason: "out_of_range".to_string(),
                    swing: false,
                    attacker_uid: uid,
                };
                messages.push(super::codec::dm_proto(uid, 6001, &miss, 2));
                return messages;
            }

            let base_dmg = (player_atk as f32 * skill.dmg_multiplier) as i32;
            let dmg = (base_dmg - target.total_def()).max(1);
            let crit = (uid + now) % 5 == 0;
            let final_dmg = if crit { dmg * 2 } else { dmg };

            target.hp = (target.hp - final_dmg).max(0);
            let target_hp = target.hp;
            let target_max_hp = target.max_hp;
            let target_name = target.name.clone();

            // 给攻击者发战斗结果 (proto)
            let battle = gp::CombatResult {
                target_uid,
                damage: final_dmg,
                target_hp,
                crit,
                miss: false,
                player_hp: 0,
                exp_gained: 0,
                error: String::new(),
                cooldown_left: 0,
                target_name,
                skill_id,
                reason: String::new(),
                swing: false,
                attacker_uid: uid,
            };
            messages.push(super::codec::dm_proto(uid, 6001, &battle, 2));

            // 给被攻击者发受击通知 — 保持 JSON：含 attackerName/maxHp 等 proto 未定义的字段
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

                // 自动复活 — 复活消息保持 JSON：只发部分字段
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

        // 无目标 - 空挥 (proto: CombatResult with swing=true)
        let swing_msg = gp::CombatResult {
            target_uid: 0,
            damage: (player_atk as f32 * skill.dmg_multiplier) as i32,
            target_hp: 0,
            crit: false,
            miss: false,
            player_hp: 0,
            exp_gained: 0,
            error: String::new(),
            cooldown_left: 0,
            target_name: String::new(),
            skill_id,
            reason: String::new(),
            swing: true,
            attacker_uid: uid,
        };
        messages.push(super::codec::dm_proto(uid, 6001, &swing_msg, 2));

        messages
    }
}
