// ════════════════════════════════════════════════════════════════
// 逻辑服实现 — 世界/NPC/怪物AI (impl GameState)
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use super::state::*;
use super::types::*;
use super::utils::*;
use rust_mmo_gate::grpc_router::proto::gate::*;
use serde_json::Value;

impl GameState {
    // ════════════════════════════════════════════════════════════
    // NPC交互
    // ════════════════════════════════════════════════════════════
    pub fn handle_npc_interact(&self, uid: u64, npc_id: u32) -> Vec<DownstreamMessage> {
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
                // NPC 4 铁匠·孙七: 额外提供装备强化选项
                if npc.id == 4 {
                    options.push(serde_json::json!({
                        "type": "enhance_weapon",
                        "label": "强化武器",
                    }));
                    options.push(serde_json::json!({
                        "type": "enhance_armor",
                        "label": "强化防具",
                    }));
                    options.push(serde_json::json!({
                        "type": "enhance_accessory",
                        "label": "强化饰品",
                    }));
                }
                options.push(serde_json::json!({
                    "type": "shop",
                    "label": "查看商品",
                }));
            }
            _ => {}
        }

        let mut dialog_data = serde_json::json!({
            "npcId": npc.id,
            "name": npc.name,
            "dialog": npc.dialog,
            "type": npc.npc_type,
            "options": options,
        });

        // v0.6: 商人附加商品列表
        if npc.npc_type == "merchant" {
            let shop_items: Vec<serde_json::Value> = SHOP_ITEMS.iter().map(|s| {
                let def = get_item_def(s.item_id);
                serde_json::json!({
                    "itemId": s.item_id,
                    "name": def.map(|d| d.name).unwrap_or("?"),
                    "icon": def.map(|d| d.icon).unwrap_or("?"),
                    "price": s.price,
                    "sellPrice": s.sell_price,
                    "stock": s.stock,
                })
            }).collect();
            dialog_data["shop"] = serde_json::json!(shop_items);
        }

        let dialog_json = dialog_data.to_string();
        messages.push(dm(uid, 5006, dialog_json, 1));

        // 治疗师直接治疗
        // v0.6: 传送门 — 切换地图
        if npc.npc_type.starts_with("portal") {
            let target_map: u32 = if npc.npc_type == "portal" {
                // 新手村的传送门 → 按NPC ID决定目标
                match npc.id {
                    6 => 2,  // 森林
                    7 => 3,  // 沙漠
                    8 => 4,  // 地下城
                    _ => 1,
                }
            } else {
                // portal_mapX → 返回新手村
                1
            };
            if let Some(mut p) = self.players.get_mut(&uid) {
                p.current_map = target_map;
                // 传送后发送完整状态和实体列表
                let map = MAP_DEFS.iter().find(|m| m.id == target_map).unwrap();
                let teleport_json = serde_json::json!({
                    "uid": uid,
                    "mapId": target_map,
                    "mapName": map.name,
                    "bgColor": map.bg_color,
                    "x": 400.0, "y": 400.0,
                    "hp": p.hp, "maxHp": p.max_hp,
                    "mp": p.mp, "maxMp": p.max_mp,
                    "transported": true,
                }).to_string();
                messages.push(dm(uid, 5001, teleport_json, 2));
            }
        }

        // v0.6: 地下城 — 生成Boss怪物
        if npc.npc_type == "dungeon" || npc.npc_type.starts_with("dungeon") {
            let boss_id: u32 = match npc.npc_type.as_str() {
                "dungeon" | "dungeon2" => 6,
                "dungeon3" => 7,
                _ => 8,
            };
            // 检查Boss是否已存在
            let boss_alive = self.mobs.iter().any(|m| m.value().def_id == boss_id);
            if boss_alive {
                let warn_json = serde_json::json!({"name":npc.name,"dialog":"Boss 还活着! 先击败它才能再次召唤。","type":"dungeon"}).to_string();
                messages.push(dm(uid, 5006, warn_json, 0));
            } else {
                // 生成Boss (生成在NPC旁边)
                let boss_x = npc.x + 40.0;
                let boss_y = npc.y;
                let entity_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mdef = get_mob_def(boss_id).unwrap();
                let mob = MobEntity {
                    entity_id,
                    def_id: boss_id,
                    name: mdef.name.to_string(),
                    x: boss_x, y: boss_y,
                    spawn_x: boss_x, spawn_y: boss_y,
                    dir: 2,
                    hp: mdef.max_hp,
                    max_hp: mdef.max_hp,
                    atk: mdef.atk,
                    def: mdef.def,
                    level: mdef.level,
                    exp: mdef.exp,
                    state: MobState::Idle,
                    target_uid: None,
                    last_attack: current_millis(),
                    last_move: current_millis(),
                    move_dir: 0.0,
                    patrol_tx: Some(boss_x + mdef.radius),
                    patrol_ty: Some(boss_y),
                };
                self.mobs.insert(entity_id, mob);
                let name = get_mob_def(boss_id).map(|d| d.name).unwrap_or("Boss");
                let spawn_json = serde_json::json!({"name":npc.name,"dialog":format!("{} 被召唤出来了!", name),"type":"dungeon","bossId":boss_id}).to_string();
                messages.push(dm(uid, 5006, spawn_json, 0));
            }
        }


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
    // 怪物AI Tick (同步，由查询触发)
    // 修复 DashMap 锁竞争：先收集ID，逐个获取写锁，避免持有全部分片锁
    // ════════════════════════════════════════════════════════════
    pub fn tick_mob_ai(&self, _querying_uid: u64) {
        let now = current_millis();

        // 收集所有玩家位置 (短持锁，collect 后释放)
        let player_positions: Vec<(u64, f32, f32)> = self
            .players
            .iter()
            .map(|p| (p.uid, p.x, p.y))
            .collect();

        // 先收集所有怪物ID，避免 iter_mut 持有全部分片写锁
        let mob_ids: Vec<u64> = self.mobs.iter().map(|m| m.entity_id).collect();

        for eid in mob_ids {
            // 逐个获取怪物写锁，循环结束时自动释放该分片锁
            let mut mob = match self.mobs.get_mut(&eid) {
                Some(m) => m,
                None => continue,
            };
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
                            // 提前计算决策，避免在持有 mob 锁时再获取 player 锁
                            let should_attack = dist <= def.attack_range
                                && now - mob.last_attack > def.attack_cd_ms;

                            if dist > def.detect_range * 2.0 {
                                mob.target_uid = None;
                                mob.state = MobState::Idle;
                            } else if dist <= def.attack_range {
                                mob.state = MobState::Attacking;
                                if should_attack {
                                    mob.last_attack = now;
                                    // 先 drop mob 锁再获取 player 写锁，避免锁嵌套
                                    let mob_atk = mob.atk;
                                    drop(mob);
                                    if let Some(mut target) = self.players.get_mut(&target_uid) {
                                        let dmg = (mob_atk - target.total_def()).max(1);
                                        target.hp = (target.hp - dmg).max(0);
                                        if target.hp == 0 {
                                            // 自动复活
                                            target.hp = target.max_hp;
                                            target.mp = target.max_mp;
                                        }
                                    }
                                    // 已 drop mob，跳过后续移动逻辑
                                    continue;
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
