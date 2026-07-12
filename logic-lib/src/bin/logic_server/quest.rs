// ════════════════════════════════════════════════════════════════
// 逻辑服实现 — 任务系统 (impl GameState)
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use super::state::*;
use super::utils::*;
use logic_lib::game_proto as gp;
use rust_mmo_gate::grpc_router::proto::gate::*;

impl GameState {
    // ════════════════════════════════════════════════════════════
    // 接受任务
    // ════════════════════════════════════════════════════════════
    pub fn handle_accept_quest(&self, uid: u64, quest_id: u32) -> Vec<DownstreamMessage> {
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
            messages.push(super::codec::dm_proto(uid, 5005, &p.to_quests_proto(), 1));

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
    // 修复 DashMap 锁竞争：锁内只做修改和提取数据，锁外构建消息
    // ════════════════════════════════════════════════════════════
    pub fn handle_complete_quest(&self, uid: u64, quest_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let def = match get_quest_def(quest_id) {
            Some(d) => d,
            None => return messages,
        };

        // 在锁内完成所有修改，提取需要的数据（clone），锁外构建消息
        let result: Option<(gp::QuestUpdate, String, u32, u32, bool, gp::PlayerStats)> = {
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

                // 提取需要的数据（clone），锁外构建消息
                Some((
                    p.to_quests_proto(),
                    p.to_inventory_json(),
                    p.exp,
                    p.level,
                    leveled_up,
                    p.to_player_stats(),
                ))
            } else {
                None
            }
        };

        // 锁外构建消息
        if let Some((qu, ij, exp, level, leveled_up, sj)) = result {
            tracing::info!(uid, quest_id, "quest completed");
            messages.push(super::codec::dm_proto(uid, 5005, &qu, 1));
            messages.push(dm(uid, 5003, ij, 1));

            // 5002 经验更新 (proto: ExpUpdate with is_mp_update=false)
            let exp_update = gp::ExpUpdate {
                exp,
                max_exp: exp_for_level(level),
                level,
                gained: def.exp_reward,
                mp: 0,
                max_mp: 0,
                is_mp_update: false,
            };
            messages.push(super::codec::dm_proto(uid, 5002, &exp_update, 1));

            if leveled_up {
                messages.push(super::codec::dm_proto(uid, 5001, &sj, 2));
                let broadcast = serde_json::json!({
                    "from": 0,
                    "fromName": "System",
                    "text": format!("Player{} 升到了 {} 级!", uid, level),
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
}
