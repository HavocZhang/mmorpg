// ════════════════════════════════════════════════════════════════
// 逻辑服实现 — 消息入口与分发 (impl GameState)
// process_message 是唯一入口，各领域 handler 分散在:
//   combat.rs / inventory.rs / quest.rs / world.rs
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use super::state::*;
use super::types::*;
use super::utils::*;
use logic_lib::game_proto as gp;
use rust_mmo_gate::grpc_router::proto::gate::*;
use serde_json::Value;

impl GameState {
    pub fn process_message(&self, uid: u64, msg_id: u32, payload: &[u8]) -> ForwardResponse {
        let mut messages = Vec::new();
        let upstream = super::codec::decode_upstream(msg_id, payload);
        // 对未迁移的消息，提取 JSON 供后续 handler 使用
        let json = match &upstream {
            super::codec::UpstreamMsg::JsonFallback(v) => v.clone(),
            _ => Value::Null,
        };
        let payload_str = String::from_utf8_lossy(payload);

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

            // ── 请求配置数据 (v0.8) ── 下发 9100 配置 JSON
            101 => {
                let config = super::config_loader::get_config();
                messages.push(dm(uid, 9100, config.to_json(), 0));
            }

            // ── 战斗：基础攻击 ──
            1001 => {
                let target_uid = match &upstream {
                    super::codec::UpstreamMsg::AttackRequest(m) => m.target_uid,
                    _ => json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0),
                };
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
                let (skill_id, target_uid) = match &upstream {
                    super::codec::UpstreamMsg::SkillAttackRequest(m) => (m.skill_id, m.target_uid),
                    _ => {
                        // JSON fallback（旧客户端）
                        let skill_id = json.get("skillId").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                        let target_uid = json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0);
                        (skill_id, target_uid)
                    }
                };
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
                let drop_id = match &upstream {
                    super::codec::UpstreamMsg::PickupRequest(m) => m.drop_id,
                    _ => json.get("dropId").and_then(|v| v.as_u64()).unwrap_or(0),
                };
                messages.extend(self.handle_pickup(uid, drop_id));
            }

            // ── 装备/卸下 ──
            1004 => {
                let item_id = match &upstream {
                    super::codec::UpstreamMsg::EquipRequest(m) => m.item_id,
                    _ => json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                };
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
                let quest_id = match &upstream {
                    super::codec::UpstreamMsg::AcceptQuestRequest(m) => m.quest_id,
                    _ => json.get("questId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                };
                messages.extend(self.handle_accept_quest(uid, quest_id));
            }

            // ── 完成任务 ──
            1006 => {
                let quest_id = match &upstream {
                    super::codec::UpstreamMsg::CompleteQuestRequest(m) => m.quest_id,
                    _ => json.get("questId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                };
                messages.extend(self.handle_complete_quest(uid, quest_id));
            }

            // ── NPC交互 ──
            1007 => {
                let npc_id = match &upstream {
                    super::codec::UpstreamMsg::NpcInteractRequest(m) => m.npc_id,
                    _ => json.get("npcId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                };
                messages.extend(self.handle_npc_interact(uid, npc_id));
            }

            // ── 使用物品 ──
            1008 => {
                let item_id = match &upstream {
                    super::codec::UpstreamMsg::UseItemRequest(m) => m.item_id,
                    _ => json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                };
                // ── 反外挂: 背包校验 ──
                if let Some(player) = self.players.get(&uid) {
                    if !player.inventory.iter().any(|(id, c)| *id == item_id && *c > 0) {
                        tracing::warn!("反外挂: 使用不存在的物品 uid={} item={}", uid, item_id);
                        return ForwardResponse { messages: vec![dm(uid, 6001, serde_json::json!({"error": "item_not_found"}).to_string(), 0)] };
                    }
                }
                messages.extend(self.handle_use_item(uid, item_id));
            }

            // ── 商店购买 (v0.6) ──
            1009 => {
                let (item_id, count) = match &upstream {
                    super::codec::UpstreamMsg::ShopBuyRequest(m) => (m.item_id, m.count),
                    _ => {
                        let item_id = json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let count = json.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                        (item_id, count)
                    }
                };
                messages.extend(self.handle_shop_buy(uid, item_id, count));
            }

            // ── 商店卖出 (v0.6) ──
            1010 => {
                let (item_id, count) = match &upstream {
                    super::codec::UpstreamMsg::ShopSellRequest(m) => (m.item_id, m.count),
                    _ => {
                        let item_id = json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let count = json.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                        (item_id, count)
                    }
                };
                messages.extend(self.handle_shop_sell(uid, item_id, count));
            }

            // ── 装备强化 (v0.7) ──
            1011 => {
                let slot = match &upstream {
                    super::codec::UpstreamMsg::EnhanceRequest(m) => m.slot.clone(),
                    _ => json.get("slot").and_then(|v| v.as_str()).unwrap_or("weapon").to_string(),
                };
                messages.extend(self.handle_enhance(uid, &slot));
            }

            // ── 队伍邀请 ──
            2002 => {
                let target = match &upstream {
                    super::codec::UpstreamMsg::PartyInviteRequest(m) => m.target_uid,
                    _ => json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0),
                };
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

            // ── 聊天 (2001 走 proto，其余 2xxx 仍走 JSON) ──
            2001 => {
                let (text, channel) = match &upstream {
                    super::codec::UpstreamMsg::ChatRequest(m) => (m.text.clone(), m.channel.clone()),
                    _ => {
                        let text = json.get("text").and_then(|v| v.as_str()).unwrap_or(&payload_str).to_string();
                        let channel = json.get("channel").and_then(|v| v.as_str()).unwrap_or("world").to_string();
                        (text, channel)
                    }
                };

                let from_name = self
                    .players
                    .get(&uid)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| format!("Player{}", uid));

                let ack_json = serde_json::json!({ "msgId": msg_id }).to_string();
                messages.push(dm(uid, 7001, ack_json, 1));

                // v0.6: 私聊 — ChatRequest proto 无 target_uid 字段，从原始 payload 解析（兼容 JSON 客户端）
                if channel == "private" {
                    let target = serde_json::from_slice::<Value>(payload)
                        .ok()
                        .and_then(|v| v.get("targetUid").and_then(|x| x.as_u64()))
                        .unwrap_or(0);
                    if target > 0 && self.players.contains_key(&target) {
                        let whisp = serde_json::json!({
                            "from": uid,
                            "fromName": from_name,
                            "text": text,
                            "channel": "private",
                        }).to_string();
                        messages.push(dm(target, 7002, whisp, 1));
                    }
                } else {
                    let broadcast_json = serde_json::json!({
                        "from": uid,
                        "fromName": from_name,
                        "text": text,
                        "channel": channel,
                    }).to_string();
                    messages.push(dm(0, 7002, broadcast_json, 1));
                }
            }

            // ── 聊天 (其余 2xxx 消息仍走 JSON) ──
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

                // v0.6: 私聊 — 发送到指定目标而非广播
                let channel = json.get("channel").and_then(|v| v.as_str()).unwrap_or("world");
                if channel == "private" {
                    let target = json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0);
                    if target > 0 && self.players.contains_key(&target) {
                        let whisp = serde_json::json!({
                            "from": uid,
                            "fromName": from_name,
                            "text": text,
                            "channel": "private",
                        }).to_string();
                        messages.push(dm(target, 7002, whisp, 1));
                    }
                } else {
                    let broadcast_json = serde_json::json!({
                        "from": uid,
                        "fromName": from_name,
                        "text": text,
                        "channel": channel,
                    }).to_string();
                    messages.push(dm(0, 7002, broadcast_json, 1));
                }
            }

            // ── 公会系统 (v0.6) ──
            2501 => { // 创建公会
                let gname = json.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if gname.is_empty() {
                    messages.push(dm(uid, 7001, r#"{"error":"公会名不能为空"}"#.to_string(), 0));
                } else if self.guilds.contains_key(&gname) {
                    messages.push(dm(uid, 7001, format!(r#"{{"error":"公会 {} 已存在"}}"#, gname), 0));
                } else if self.player_guild.contains_key(&uid) {
                    messages.push(dm(uid, 7001, r#"{"error":"你已在公会中"}"#.to_string(), 0));
                } else {
                    let name = self.players.get(&uid).map(|p| p.name.clone()).unwrap_or("?".into());
                    self.guilds.insert(gname.clone(), GuildInfo { name: gname.clone(), leader: uid, members: vec![uid], funds: 0, created_at: current_millis() });
                    self.player_guild.insert(uid, gname.clone());
                    messages.push(dm(uid, 7001, format!(r#"{{"type":"guild_created","name":"{}","leader":"{}"}}"#, gname, name), 1));
                }
            }
            2502 => { // 加入公会
                let gname = json.get("guildName").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if let Some(mut g) = self.guilds.get_mut(&gname) {
                    if !g.members.contains(&uid) {
                        g.members.push(uid);
                        self.player_guild.insert(uid, gname.clone());
                        let name = self.players.get(&uid).map(|p| p.name.clone()).unwrap_or("?".into());
                        messages.push(dm(uid, 7001, format!(r#"{{"type":"guild_joined","name":"{}"}}"#, gname), 1));
                        for &mid in &g.members {
                            if mid != uid { messages.push(dm(mid, 7002, serde_json::json!({"type":"guild_member_join","name":name}).to_string(), 1)); }
                        }
                    }
                } else {
                    messages.push(dm(uid, 7001, r#"{"error":"公会不存在"}"#.to_string(), 0));
                }
            }
            2503 => { // 离开公会
                if let Some((_, gname)) = self.player_guild.remove(&uid) {
                    if let Some(mut g) = self.guilds.get_mut(&gname) {
                        g.members.retain(|&m| m != uid);
                        let name = self.players.get(&uid).map(|p| p.name.clone()).unwrap_or("?".into());
                        messages.push(dm(uid, 7001, format!(r#"{{"type":"guild_left","name":"{}"}}"#, gname), 1));
                        for &mid in &g.members {
                            messages.push(dm(mid, 7002, serde_json::json!({"type":"guild_member_leave","name":name}).to_string(), 1));
                        }
                        if g.members.is_empty() { self.guilds.remove(&gname); }
                    }
                }
            }

            // ── PvP 决斗 (v0.6) ──
            3100 => { // 发起决斗
                let target = json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0);
                if target == uid || !self.players.contains_key(&target) {
                    messages.push(dm(uid, 7001, r#"{"error":"目标玩家不存在"}"#.to_string(), 0));
                } else {
                    self.duel_requests.insert(uid, target);
                    let cname = self.players.get(&uid).map(|p| p.name.clone()).unwrap_or("?".into());
                    messages.push(dm(target, 7002, serde_json::json!({"type":"duel_request","from":uid,"fromName":cname}).to_string(), 1));
                    messages.push(dm(uid, 7001, r#"{"type":"duel_sent"}"#.to_string(), 1));
                }
            }
            3101 => { // 接受决斗
                // Check if the player has any pending duel requests
                let target_opt = self.duel_requests.iter().find(|r| *r.value() == uid).map(|r| *r.key());
                if let Some(challenger) = target_opt {
                    self.duel_requests.remove(&challenger);
                    let cname = self.players.get(&challenger).map(|p| p.name.clone()).unwrap_or("?".into());
                    let tname = self.players.get(&uid).map(|p| p.name.clone()).unwrap_or("?".into());
                    messages.push(dm(uid, 7002, serde_json::json!({"type":"duel_start","against":challenger,"againstName":tname}).to_string(), 1));
                    messages.push(dm(challenger, 7002, serde_json::json!({"type":"duel_start","against":uid,"againstName":cname}).to_string(), 1));
                }
            }

            // ── 技能树 (v0.6) ──
            2701 => { // 选择职业
                let class_id = json.get("class").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                if class_id < 1 || class_id > 3 {
                    messages.push(dm(uid, 7001, r#"{"error":"无效职业"}"#.to_string(), 0));
                } else if let Some(mut p) = self.players.get_mut(&uid) {
                    if p.class > 0 {
                        messages.push(dm(uid, 7001, r#"{"error":"已选择职业"}"#.to_string(), 0));
                    } else {
                        p.class = class_id;
                        if let Some(cd) = CLASS_DEFS.iter().find(|c| c.id == class_id) {
                            p.atk += cd.atk_bonus;
                            p.def += cd.def_bonus;
                            p.max_hp = 100 + cd.hp_bonus;
                            p.hp = p.max_hp;
                        }
                        p.talent_pts = p.level; // 每级 1 天赋点
                        messages.push(super::codec::dm_proto(uid, 5001, &p.to_player_stats(), 2));
                        let cn = CLASS_DEFS.iter().find(|c| c.id == class_id).map(|c| c.name).unwrap_or("");
                        messages.push(dm(uid, 7001, format!(r#"{{"type":"class_chosen","class":"{}","talentPts":{}}}"#, cn, p.talent_pts), 1));
                    }
                }
            }
            2702 => { // 点天赋
                let tid = json.get("talentId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                if let Some(td) = TALENTS.iter().find(|t| t.id == tid) {
                    if let Some(mut p) = self.players.get_mut(&uid) {
                        if p.class != td.class {
                            messages.push(dm(uid, 7001, r#"{"error":"天赋不属于当前职业"}"#.to_string(), 0));
                        } else if p.talent_pts == 0 {
                            messages.push(dm(uid, 7001, r#"{"error":"天赋点不足"}"#.to_string(), 0));
                        } else if p.talents.contains(&tid) {
                            messages.push(dm(uid, 7001, r#"{"error":"已激活该天赋"}"#.to_string(), 0));
                        } else {
                            p.talent_pts -= 1;
                            p.atk += td.atk;
                            p.def += td.def;
                            p.max_hp += td.hp;
                            p.hp += td.hp;
                            p.talents.push(tid);
                            messages.push(super::codec::dm_proto(uid, 5001, &p.to_player_stats(), 2));
                            messages.push(dm(uid, 7001, format!(r#"{{"type":"talent_learned","name":"{}","talentPts":{}}}"#, td.name, p.talent_pts), 1));
                        }
                    }
                }
            }

            // ── 排行榜 (v0.7) ──
            2800 => {
                let typ = json.get("type").and_then(|v| v.as_str()).unwrap_or("level");
                let mut entries: Vec<(u64, String, u32)> = self.players.iter()
                    .map(|p| { let v = match typ { "gold" => p.gold, _ => p.level }; (p.uid, p.name.clone(), v) })
                    .collect();
                entries.sort_by(|a, b| b.2.cmp(&a.2));
                entries.truncate(20);
                let list: Vec<serde_json::Value> = entries.iter().enumerate().map(|(i, (pid, pname, val))| serde_json::json!({
                    "rank": i + 1, "uid": pid, "name": pname, "value": val
                })).collect();
                messages.push(dm(uid, 2801, serde_json::json!({"type":typ,"entries":list}).to_string(), 1));
            }

            // ── 移动 ──
            3000..=3999 => {
                let (x, y, dir) = match &upstream {
                    super::codec::UpstreamMsg::MoveRequest(m) => {
                        (m.x, m.y, m.dir as u8)
                    }
                    _ => {
                        // JSON fallback（兼容旧客户端）
                        let x = json.get("x").and_then(|v| v.as_f64()).unwrap_or(400.0) as f32;
                        let y = json.get("y").and_then(|v| v.as_f64()).unwrap_or(300.0) as f32;
                        let dir = json.get("dir").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
                        (x, y, dir)
                    }
                };

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

                let pos = gp::PlayerPosition {
                    uid,
                    x,
                    y,
                    dir: dir as u32,
                };
                messages.push(super::codec::dm_proto(0, 8001, &pos, 0));
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
                // 节流：距离上次后台 tick 不足 150ms 则跳过本次触发
                // 客户端高频查询与后台 tick 叠加会放大锁竞争
                let now = current_millis();
                let last = self.last_mob_tick.load(std::sync::atomic::Ordering::Relaxed);
                if now.saturating_sub(last) > 150 {
                    self.tick_mob_ai(uid);
                    self.last_mob_tick.store(now, std::sync::atomic::Ordering::Relaxed);
                }

                // proto 编码 EntityList (符合 game.proto 单一数据源)
                let entity_list = gp::EntityList {
                    npcs: self.npcs.iter().map(|n| n.to_entity_list_entry()).collect(),
                    mobs: self.mobs.iter().map(|m| m.to_entity_list_entry()).collect(),
                };
                messages.push(super::codec::dm_proto(uid, 9002, &entity_list, 0));
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
        // 修复锁竞争：先收集快照，再构建消息，避免持有全部分片读锁
        let mob_snapshots: Vec<(u64, f32, f32, i32, i32, String, u32, u32)> = self.mobs.iter()
            .filter(|m| m.state != MobState::Dead)
            .map(|m| (
                m.entity_id, m.x, m.y, m.hp, m.max_hp,
                m.name.clone(), m.def_id,
                get_mob_def(m.def_id).map(|d| d.level).unwrap_or(1),
            ))
            .collect();
        for (eid, x, y, hp, mhp, name, def_id, level) in mob_snapshots {
            let pos = gp::EntityPosition {
                entity_id: eid,
                x,
                y,
                hp,
                max_hp: mhp,
                name,
                def_id,
                level,
            };
            messages.push(super::codec::dm_proto(0, 8004, &pos, 0));
        }

        // 附带玩家最新 HP/MP（怪物可能已攻击）
        if let Some(p) = self.players.get(&uid) {
            messages.push(super::codec::dm_proto(uid, 5001, &p.to_player_stats(), 1));
        }

        ForwardResponse { messages }
    }
}
