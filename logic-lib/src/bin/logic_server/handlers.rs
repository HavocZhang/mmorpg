// ════════════════════════════════════════════════════════════════
// 逻辑服实现 — 业务方法 (impl GameState)
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use super::state::*;
use super::types::*;
use super::utils::*;
use logic_lib::game_proto as gp;
use rust_mmo_gate::grpc_router::proto::gate::*;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};

// 强化随机源计数器：避免快速循环里 current_millis() 相同导致 roll 固定
static ENHANCE_COUNTER: AtomicU64 = AtomicU64::new(0);

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
                        messages.push(dm(uid, 5001, p.to_stats_json(), 2));
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
                            messages.push(dm(uid, 5001, p.to_stats_json(), 2));
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

                let npcs_json: Vec<String> = self.npcs.iter().map(|n| n.to_json()).collect();
                let mobs_json: Vec<String> = self.mobs.iter().map(|m| m.to_list_entry()).collect();
                let entity_json = serde_json::json!({
                    "npcs": npcs_json,
                    "mobs": mobs_json,
                })
                .to_string();
                messages.push(dm(uid, 9002, entity_json, 0));
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
            messages.push(dm(uid, 5001, p.to_stats_json(), 1));
        }

        ForwardResponse { messages }
    }

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

        // 更新MP
        let mp_json = serde_json::json!({
            "mp": self.players.get(&uid).map(|p| p.mp).unwrap_or(0),
            "maxMp": self.players.get(&uid).map(|p| p.max_mp).unwrap_or(50),
        }).to_string();
        messages.push(dm(uid, 5002, mp_json, 1));

        // 目标是怪物实体 (先查 mobs 表)
        if target_uid >= 10000 && self.mobs.contains_key(&target_uid) {
            let mut mob = match self.mobs.get_mut(&target_uid) {
                Some(m) => m,
                None => {
                    let miss = serde_json::json!({
                        "targetUid": target_uid,
                        "dmg": 0,
                        "targetHp": 0,
                        "miss": true,
                    }).to_string();
                    messages.push(dm(uid, 6001, miss, 2));
                    return messages;
                }
            };

            if mob.state == MobState::Dead {
                let miss = serde_json::json!({
                    "targetUid": target_uid,
                    "dmg": 0,
                    "targetHp": 0,
                    "miss": true,
                }).to_string();
                messages.push(dm(uid, 6001, miss, 2));
                return messages;
            }

            // 检查距离
            let dist = distance(player_x, player_y, mob.x, mob.y);
            if dist > skill.range + 20.0 {
                let miss = serde_json::json!({
                    "targetUid": target_uid,
                    "dmg": 0,
                    "targetHp": mob.hp,
                    "miss": true,
                    "reason": "out_of_range",
                }).to_string();
                messages.push(dm(uid, 6001, miss, 2));
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
            let mob_exp = mob.exp;
            let mob_name = mob.name.clone();

            // 给攻击者发战斗结果
            let battle_json = serde_json::json!({
                "targetUid": target_uid,
                "targetName": mob_name,
                "dmg": final_dmg,
                "targetHp": mob_hp,
                "crit": crit,
                "skillId": skill_id,
            }).to_string();
            messages.push(dm(uid, 6001, battle_json, 2));

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
                // 广播死亡信息 + 掉落
                let drops = self.generate_drops(mob_def_id, mob_x, mob_y);
                let drop_json: Vec<String> = drops.iter().map(|d| d.to_json()).collect();

                let death_json = serde_json::json!({
                    "entityId": target_uid,
                    "killer": uid,
                    "killerName": format!("Player{}", uid),
                    "mobName": mob_name,
                    "drops": drop_json,
                    "exp": mob_exp,
                }).to_string();
                messages.push(dm(0, 6003, death_json, 1));

                // 插入掉落物
                for drop in &drops {
                    self.drops.insert(drop.drop_id, drop.clone());
                }

                // 给击杀者加经验和金币
                if let Some(mut p) = self.players.get_mut(&uid) {
                    let _old_level = p.level;
                    let leveled_up = p.add_exp(mob_exp);
                    // v0.6: 击杀奖励金币 = 怪物等级 * 5
                    let gold_reward = (get_mob_def(mob_def_id).map(|d| d.level).unwrap_or(1) * 5) as u32;
                    p.gold += gold_reward;

                    // 更新任务进度
                    let quest_updated = p.update_quest_progress(mob_def_id);

                    let exp_json = serde_json::json!({
                        "exp": p.exp,
                        "maxExp": exp_for_level(p.level),
                        "level": p.level,
                        "gained": mob_exp,
                    }).to_string();
                    messages.push(dm(uid, 5002, exp_json, 1));

                    if leveled_up {
                        let levelup_json = serde_json::json!({
                            "level": p.level,
                            "maxHp": p.max_hp,
                            "maxMp": p.max_mp,
                            "hp": p.hp,
                            "mp": p.mp,
                            "atk": p.total_atk(),
                            "def": p.total_def(),
                        }).to_string();
                        messages.push(dm(uid, 5001, levelup_json, 2));

                        let broadcast = serde_json::json!({
                            "from": 0,
                            "fromName": "System",
                            "text": format!("Player{} 升到了 {} 级!", uid, p.level),
                        }).to_string();
                        messages.push(dm(0, 7002, broadcast, 1));
                    }

                    if quest_updated {
                        messages.push(dm(uid, 5005, p.to_quests_json(), 1));
                    }
                }
            }

            return messages;
        }

        // 目标是玩家 (查 players 表, 不限 UID 范围)
        if target_uid > 0 && self.players.contains_key(&target_uid) {
            let mut target = match self.players.get_mut(&target_uid) {
                Some(t) => t,
                None => {
                    let miss = serde_json::json!({
                        "targetUid": target_uid,
                        "dmg": 0,
                        "targetHp": 0,
                        "miss": true,
                    }).to_string();
                    messages.push(dm(uid, 6001, miss, 2));
                    return messages;
                }
            };

            let dist = distance(player_x, player_y, target.x, target.y);
            if dist > skill.range + 20.0 {
                let miss = serde_json::json!({
                    "targetUid": target_uid,
                    "dmg": 0,
                    "targetHp": target.hp,
                    "miss": true,
                    "reason": "out_of_range",
                }).to_string();
                messages.push(dm(uid, 6001, miss, 2));
                return messages;
            }

            let base_dmg = (player_atk as f32 * skill.dmg_multiplier) as i32;
            let dmg = (base_dmg - target.total_def()).max(1);
            let crit = (uid + now) % 5 == 0;
            let final_dmg = if crit { dmg * 2 } else { dmg };

            target.hp = (target.hp - final_dmg).max(0);
            let target_hp = target.hp;
            let target_max_hp = target.max_hp;

            let battle_json = serde_json::json!({
                "targetUid": target_uid,
                "targetName": target.name,
                "dmg": final_dmg,
                "targetHp": target_hp,
                "crit": crit,
                "skillId": skill_id,
            }).to_string();
            messages.push(dm(uid, 6001, battle_json, 2));

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

                // 自动复活
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

        // 无目标 - 空挥
        let echo_json = serde_json::json!({
            "uid": uid,
            "dmg": (player_atk as f32 * skill.dmg_multiplier) as i32,
            "skillId": skill_id,
            "swing": true,
        }).to_string();
        messages.push(dm(uid, 6001, echo_json, 2));

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 拾取物品
    // ════════════════════════════════════════════════════════════
    pub fn handle_pickup(&self, uid: u64, drop_id: u64) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let drop = match self.drops.get(&drop_id) {
            Some(d) => d.clone(),
            None => {
                let err = serde_json::json!({ "error": "item_not_found" }).to_string();
                messages.push(dm(uid, 5003, err, 2));
                return messages;
            }
        };

        // 检查距离
        let player_pos = self.players.get(&uid).map(|p| (p.x, p.y));
        if let Some((px, py)) = player_pos {
            if distance(px, py, drop.x, drop.y) > 60.0 {
                let err = serde_json::json!({ "error": "too_far" }).to_string();
                messages.push(dm(uid, 5003, err, 2));
                return messages;
            }
        }

        self.drops.remove(&drop_id);

        // 添加到背包
        if let Some(mut p) = self.players.get_mut(&uid) {
            p.add_item(drop.item_id, drop.count);
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
        }

        // 广播掉落物被拾取
        let pickup_json = serde_json::json!({
            "dropId": drop_id,
            "pickedBy": uid,
        }).to_string();
        messages.push(dm(0, 6003, pickup_json, 1));

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 装备/卸下
    // ════════════════════════════════════════════════════════════
    pub fn handle_equip(&self, uid: u64, item_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let item = match get_item_def(item_id) {
            Some(i) => i,
            None => return messages,
        };

        if item.item_type == "potion" || item.item_type == "material" {
            let err = serde_json::json!({ "error": "cannot_equip" }).to_string();
            messages.push(dm(uid, 5004, err, 2));
            return messages;
        }

        if let Some(mut p) = self.players.get_mut(&uid) {
            // 检查背包是否有该物品
            if !p.inventory.iter().any(|(id, c)| *id == item_id && *c > 0) {
                let err = serde_json::json!({ "error": "not_in_inventory" }).to_string();
                messages.push(dm(uid, 5004, err, 2));
                return messages;
            }

            let slot = match item.item_type {
                "weapon" => &mut p.weapon,
                "armor" => &mut p.armor,
                "accessory" => &mut p.accessory,
                _ => return messages,
            };

            // 交换装备：旧的放回背包，新的装备上
            let old = *slot;
            *slot = Some(item_id);

            // 从背包移除新装备的物品
            if let Some(entry) = p.inventory.iter_mut().find(|(id, _)| *id == item_id) {
                entry.1 -= 1;
            }

            // 旧装备放回背包
            if let Some(old_id) = old {
                p.add_item(old_id, 1);
            }

            // 发送更新
            messages.push(dm(uid, 5004, p.to_equipment_json(), 1));
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(dm(uid, 5001, p.to_stats_json(), 1));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 装备强化 (v0.7)
    // slot: "weapon" | "armor" | "accessory"
    // ════════════════════════════════════════════════════════════
    pub fn handle_enhance(&self, uid: u64, slot: &str) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        // 锁内完成判定和修改，提取结果，锁外构建消息
        let result: Option<(u32, u32, bool, String)> = {
            let mut p = match self.players.get_mut(&uid) {
                Some(p) => p,
                None => return messages,
            };

            let (item_id, enhance) = match slot {
                "weapon" => (p.weapon, p.weapon_enhance),
                "armor" => (p.armor, p.armor_enhance),
                "accessory" => (p.accessory, p.accessory_enhance),
                _ => return messages,
            };

            let item_id = match item_id {
                Some(id) => id,
                None => {
                    let err = serde_json::json!({"error":"no_item","slot":slot}).to_string();
                    return vec![dm(uid, 5004, err, 2)];
                }
            };

            // 上限检查
            if enhance >= 10 {
                let err = serde_json::json!({"error":"max_level","slot":slot}).to_string();
                return vec![dm(uid, 5004, err, 2)];
            }

            // 费用 = (当前等级+1) * 100
            let cost = (enhance + 1) * 100;
            if p.gold < cost {
                let err = serde_json::json!({"error":"insufficient_gold","need":cost,"have":p.gold}).to_string();
                return vec![dm(uid, 5004, err, 2)];
            }

            // 成功率: +1~+3=100%, +4~+6=80%, +7~+9=50%, +10=20%
            let success_rate = match enhance + 1 {
                1..=3 => 1.0,
                4..=6 => 0.8,
                7..=9 => 0.5,
                10 => 0.2,
                _ => 1.0,
            };
            // 随机源: uid + 时间 + 自增计数器，避免快速循环里时间相同导致 roll 固定
            let counter = ENHANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let now = current_millis();
            let seed = uid
                .wrapping_mul(2654435761)
                .wrapping_add(now)
                .wrapping_add(counter.wrapping_mul(11400714819323198485));
            let roll = (seed % 10000) as f32 / 10000.0;
            let success = roll < success_rate;

            // 扣金币
            p.gold -= cost;

            let item_def_name = get_item_def(item_id)
                .map(|d| d.name)
                .unwrap_or("装备")
                .to_string();

            if success {
                match slot {
                    "weapon" => p.weapon_enhance += 1,
                    "armor" => p.armor_enhance += 1,
                    "accessory" => p.accessory_enhance += 1,
                    _ => {}
                }
                Some((enhance + 1, p.gold, true, item_def_name))
            } else {
                Some((enhance, p.gold, false, item_def_name))
            }
        };

        // 锁外构建消息
        if let Some((level, gold, success, item_name)) = result {
            let equip_json = self
                .players
                .get(&uid)
                .map(|p| p.to_equipment_json())
                .unwrap_or_default();
            messages.push(dm(uid, 5004, equip_json, 1));

            let stats_json = self
                .players
                .get(&uid)
                .map(|p| p.to_stats_json())
                .unwrap_or_default();
            messages.push(dm(uid, 5001, stats_json, 1));

            let msg = if success {
                format!(
                    "强化成功! {} +{} (消耗 {} 金币)",
                    item_name,
                    level,
                    level * 100
                )
            } else {
                format!(
                    "强化失败... {} 仍为 +{} (消耗 {} 金币)",
                    item_name,
                    level,
                    (level + 1) * 100
                )
            };
            let result_json = serde_json::json!({
                "type": "enhance_result",
                "success": success,
                "slot": slot,
                "level": level,
                "gold": gold,
                "message": msg,
            })
            .to_string();
            messages.push(dm(uid, 5006, result_json, 1));
        }

        messages
    }

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
            messages.push(dm(uid, 5005, p.to_quests_json(), 1));

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
        let result: Option<(String, String, u32, u32, bool, String)> = {
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
                    p.to_quests_json(),
                    p.to_inventory_json(),
                    p.exp,
                    p.level,
                    leveled_up,
                    p.to_stats_json(),
                ))
            } else {
                None
            }
        };

        // 锁外构建消息
        if let Some((qj, ij, exp, level, leveled_up, sj)) = result {
            tracing::info!(uid, quest_id, "quest completed");
            messages.push(dm(uid, 5005, qj, 1));
            messages.push(dm(uid, 5003, ij, 1));

            let exp_json = serde_json::json!({
                "exp": exp,
                "maxExp": exp_for_level(level),
                "level": level,
                "gained": def.exp_reward,
            }).to_string();
            messages.push(dm(uid, 5002, exp_json, 1));

            if leveled_up {
                messages.push(dm(uid, 5001, sj, 2));
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
    // 使用物品
    // ════════════════════════════════════════════════════════════
    pub fn handle_use_item(&self, uid: u64, item_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let item = match get_item_def(item_id) {
            Some(i) => i,
            None => return messages,
        };

        if item.item_type != "potion" {
            let err = serde_json::json!({ "error": "cannot_use" }).to_string();
            messages.push(dm(uid, 5003, err, 2));
            return messages;
        }

        if let Some(mut p) = self.players.get_mut(&uid) {
            if !p.remove_item(item_id, 1) {
                let err = serde_json::json!({ "error": "not_in_inventory" }).to_string();
                messages.push(dm(uid, 5003, err, 2));
                return messages;
            }

            if item.hp_restore > 0 {
                p.hp = (p.hp + item.hp_restore).min(p.max_hp);
            }
            if item.mp_restore > 0 {
                p.mp = (p.mp + item.mp_restore).min(p.max_mp);
            }

            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(dm(uid, 5001, p.to_stats_json(), 1));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 商店系统 (v0.6)
    // ════════════════════════════════════════════════════════════

    /// 从商店购买物品
    pub fn handle_shop_buy(&self, uid: u64, item_id: u32, count: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();
        // 查找商品
        let shop_item = match SHOP_ITEMS.iter().find(|s| s.item_id == item_id) {
            Some(s) => s,
            None => {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":"该物品不在商店中","type":"merchant"}).to_string(), 0));
                return messages;
            }
        };
        // 检查库存
        if let Some(stock) = shop_item.stock {
            if count > stock {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":"库存不足!","type":"merchant"}).to_string(), 0));
                return messages;
            }
        }
        let total_cost = shop_item.price * count;
        // 扣金币 + 加物品
        if let Some(mut p) = self.players.get_mut(&uid) {
            if p.gold < total_cost {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":format!("金币不足! 需要 {} 金", total_cost),"type":"merchant"}).to_string(), 0));
                return messages;
            }
            p.gold -= total_cost;
            // 添加物品到背包
            if let Some(pos) = p.inventory.iter().position(|(id, _)| *id == item_id) {
                p.inventory[pos].1 += count as u32;
            } else {
                p.inventory.push((item_id, count as u32));
            }
            let item_name = get_item_def(item_id).map(|d| d.name).unwrap_or("物品");
            messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":format!("购买 {} x{} 成功! 花费 {} 金币", item_name, count, total_cost),"type":"merchant"}).to_string(), 0));
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(dm(uid, 5001, p.to_stats_json(), 1));
        }
        messages
    }

    /// 出售物品给商店
    pub fn handle_shop_sell(&self, uid: u64, item_id: u32, count: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();
        let shop_item = match SHOP_ITEMS.iter().find(|s| s.item_id == item_id) {
            Some(s) => s,
            None => {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":"商店不收这种物品","type":"merchant"}).to_string(), 0));
                return messages;
            }
        };
        let total_earned = shop_item.sell_price * count;
        if let Some(mut p) = self.players.get_mut(&uid) {
            // 检查是否有足够数量
            if let Some(pos) = p.inventory.iter().position(|(id, c)| *id == item_id && *c >= count as u32) {
                p.inventory[pos].1 -= count as u32;
                if p.inventory[pos].1 == 0 {
                    p.inventory.remove(pos);
                }
            } else {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":"背包中数量不足!","type":"merchant"}).to_string(), 0));
                return messages;
            }
            p.gold += total_earned;
            let item_name = get_item_def(item_id).map(|d| d.name).unwrap_or("物品");
            messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":format!("出售 {} x{} 成功! 获得 {} 金币", item_name, count, total_earned),"type":"merchant"}).to_string(), 0));
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(dm(uid, 5001, p.to_stats_json(), 1));
        }
        messages
    }

    // ════════════════════════════════════════════════════════════
    // 生成掉落物
    // ════════════════════════════════════════════════════════════
    pub fn generate_drops(&self, mob_def_id: u32, x: f32, y: f32) -> Vec<ItemDrop> {
        let mut drops = Vec::new();
        let now = current_millis();

        match mob_def_id {
            1 => { // 史莱姆
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 9, x: x + 10.0, y: y + 10.0, count: 1 });
                if now % 3 == 0 {
                    let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    drops.push(ItemDrop { drop_id, item_id: 6, x: x - 10.0, y: y + 5.0, count: 1 });
                }
            }
            2 => { // 哥布林
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 10, x: x + 10.0, y: y + 10.0, count: 1 });
                if now % 2 == 0 {
                    let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    drops.push(ItemDrop { drop_id, item_id: 7, x: x - 10.0, y: y + 5.0, count: 1 });
                }
            }
            3 => { // 骷髅战士
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 1, x: x + 10.0, y: y + 10.0, count: 1 });
                if now % 2 == 0 {
                    let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    drops.push(ItemDrop { drop_id, item_id: 6, x: x - 10.0, y: y + 5.0, count: 1 });
                }
            }
            4 => { // 暗影法师
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 2, x: x + 10.0, y: y + 10.0, count: 1 });
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x - 10.0, y: y + 5.0, count: 1 });
            }
            5 => { // 岩石巨人
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 4, x: x + 10.0, y: y + 10.0, count: 1 });
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 5, x: x - 10.0, y: y + 5.0, count: 1 });
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x + 5.0, y: y - 10.0, count: 1 });
            }
            // v0.6: Boss 掉落 (全部高价值物品)
            6 => { // 森林守护者
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 2, x: x + 10.0, y: y, count: 1 }); // 钢剑
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 4, x: x - 10.0, y: y, count: 1 }); // 铁甲
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x, y: y + 10.0, count: 3 }); // 全恢复x3
            }
            7 => { // 沙虫领主
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 2, x: x + 10.0, y: y, count: 1 }); // 钢剑
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 5, x: x - 10.0, y: y, count: 2 }); // 力量戒指x2
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x, y: y + 10.0, count: 2 }); // 全恢复x2
            }
            8 => { // 暗黑巫妖王
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 2, x: x + 10.0, y: y, count: 1 }); // 钢剑
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 4, x: x - 10.0, y: y, count: 1 }); // 铁甲
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 5, x: x + 5.0, y: y - 10.0, count: 3 }); // 戒指x3
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x - 5.0, y: y - 10.0, count: 5 }); // 全恢复x5
            }
            _ => {}
        }

        drops
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
