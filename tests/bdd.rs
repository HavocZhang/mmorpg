//! BDD 测试入口 — cucumber harness
//!
//! 运行: cargo test --test bdd
//!
//! 覆盖 7 个 .feature 文件全部场景：
//! - connect.feature: TCP连接与握手鉴权
//! - session.feature: 会话Session管理
//! - protocol.feature: 私有协议编解码
//! - message.feature: 消息收发与团战削峰
//! - security.feature: 限流与安全防护
//! - cluster.feature: 集群跨网关协作
//! - shutdown.feature: 容灾与优雅启停

#[path = "bdd/steps/mod.rs"]
mod steps;

/// 场景服模拟状态（BDD 测试用）
#[derive(Debug, Default)]
pub struct SceneState {
    pub maps: std::collections::HashMap<String, SceneMapData>,
    pub player_map: std::collections::HashMap<u64, String>,
    pub player_pos: std::collections::HashMap<u64, (f64, f64)>,
    pub aoi_radius: f64,
    pub max_speed: f64,
    pub npcs: std::collections::HashMap<String, Vec<SceneNpcData>>,
    pub enter_events: Vec<(u64, u64)>,
    pub leave_events: Vec<(u64, u64)>,
    pub move_broadcasts: Vec<(u64, u64)>,
    pub move_confirmations: Vec<u64>,
    pub speed_violations: Vec<u64>,
    pub boundary_violations: Vec<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SceneMapData {
    pub name: String,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone)]
pub struct SceneNpcData {
    pub id: u64,
    pub name: String,
    pub x: f64,
    pub y: f64,
}

impl SceneState {
    pub fn new() -> Self {
        Self { aoi_radius: 300.0, max_speed: 800.0, ..Default::default() }
    }

    pub fn load_map(&mut self, name: &str, w: f64, h: f64) {
        self.maps.insert(name.into(), SceneMapData { name: name.into(), width: w, height: h });
    }

    pub fn join_map(&mut self, uid: u64, map_name: &str, x: f64, y: f64) -> Result<(), String> {
        let map = self.maps.get(map_name).ok_or("地图不存在".to_string())?;
        let cx = x.max(0.0).min(map.width);
        let cy = y.max(0.0).min(map.height);
        self.player_map.insert(uid, map_name.into());
        self.player_pos.insert(uid, (cx, cy));
        Ok(())
    }

    pub fn leave_map(&mut self, uid: u64) {
        self.player_map.remove(&uid);
        self.player_pos.remove(&uid);
    }

    pub fn move_player(&mut self, uid: u64, x: f64, y: f64, dt: f64) -> Result<(), String> {
        let mname = self.player_map.get(&uid).ok_or("玩家不在地图中")?.clone();
        let map = self.maps.get(&mname).unwrap();
        let cx = x.max(0.0).min(map.width);
        let cy = y.max(0.0).min(map.height);
        if cx != x || cy != y { self.boundary_violations.push(uid); }

        // 保存移动前位置（用于AOI离开检测）
        let old_pos = self.player_pos.get(&uid).copied();

        if let Some(&(ox, oy)) = self.player_pos.get(&uid) {
            let dist = ((cx - ox).powi(2) + (cy - oy).powi(2)).sqrt();
            if dt > 0.0 && dist / dt > self.max_speed {
                self.speed_violations.push(uid);
                let r = self.max_speed * dt / dist;
                self.player_pos.insert(uid, (ox + (cx - ox) * r, oy + (cy - oy) * r));
                return Ok(());
            }
        }
        self.player_pos.insert(uid, (cx, cy));
        self.move_confirmations.push(uid);

        let uids: Vec<u64> = self.player_map.iter()
            .filter(|(_, m)| m.as_str() == mname.as_str()).map(|(u, _)| *u).collect();
        for &oid in &uids {
            if oid == uid { continue; }
            if let Some(&(ox, oy)) = self.player_pos.get(&oid) {
                let new_dist = ((cx - ox).powi(2) + (cy - oy).powi(2)).sqrt();
                let was_in_range = old_pos.is_some_and(|(px, py)| {
                    ((px - ox).powi(2) + (py - oy).powi(2)).sqrt() <= self.aoi_radius
                });
                if new_dist <= self.aoi_radius {
                    self.enter_events.push((uid, oid));
                    self.enter_events.push((oid, uid));
                    self.move_broadcasts.push((uid, oid));
                } else if was_in_range {
                    // 离开AOI：从在范围内变为在范围外
                    self.leave_events.push((uid, oid));
                    self.leave_events.push((oid, uid));
                }
            }
        }
        Ok(())
    }

    pub fn teleport(&mut self, uid: u64, map_name: &str, x: f64, y: f64) -> Result<(), String> {
        self.leave_map(uid);
        self.join_map(uid, map_name, x, y)
    }

    pub fn spawn_npc(&mut self, m: &str, id: u64, name: &str, x: f64, y: f64) {
        self.npcs.entry(m.into()).or_default().push(SceneNpcData { id, name: name.into(), x, y });
    }

    pub fn get_visible_npcs(&self, uid: u64) -> Vec<String> {
        let m = match self.player_map.get(&uid) { Some(m) => m, None => return vec![] };
        let (px, py) = match self.player_pos.get(&uid) { Some(p) => *p, None => return vec![] };
        self.npcs.get(m).unwrap_or(&vec![]).iter()
            .filter(|n| ((px - n.x).powi(2) + (py - n.y).powi(2)).sqrt() <= self.aoi_radius)
            .map(|n| n.name.clone()).collect()
    }

    pub fn is_in_range(&self, a: u64, b: u64) -> bool {
        let p1 = match self.player_pos.get(&a) { Some(p) => *p, None => return false };
        let p2 = match self.player_pos.get(&b) { Some(p) => *p, None => return false };
        ((p1.0 - p2.0).powi(2) + (p1.1 - p2.1).powi(2)).sqrt() <= self.aoi_radius
    }

    pub fn map_player_count(&self, map_name: &str) -> usize {
        self.player_map.iter().filter(|(_, m)| m.as_str() == map_name).count()
    }
}

/// 战斗服模拟状态（BDD 测试用）
#[derive(Debug, Default)]
pub struct CombatState {
    pub entities: std::collections::HashMap<u64, CombatEntityState>,
    pub battle_results: Vec<CombatResult>,
    pub death_events: Vec<CombatDeathEvent>,
    pub buffs: std::collections::HashMap<u64, Vec<CombatBuff>>,
    pub xp_events: Vec<CombatXpEvent>,
    pub last_damage: Option<i64>,
    pub last_action: Option<String>,
    /// entity_id -> entity_id -> bool
    pub combat_relations: std::collections::HashMap<u64, std::collections::HashSet<u64>>,
    pub combat_states: std::collections::HashMap<u64, String>,
    pub broadcast_targets: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct CombatEntityState {
    pub entity_type: String, // "player" or "monster"
    pub hp: i64,
    pub max_hp: i64,
    pub atk: i64,
    pub def: i64,
    pub crit_rate: f64,    // percentage 0-100
    pub crit_dmg: f64,     // multiplier
    pub level: u32,
    pub xp: u64,
    pub xp_to_level: u64,
    pub kill_xp: u64,      // XP awarded when killed
    pub alive: bool,
    pub xp_events_recorded: Vec<i64>,
    pub level_ups: u32,
}

#[derive(Debug, Clone)]
pub struct CombatResult {
    pub attacker_id: u64,
    pub target_id: u64,
    pub damage: i64,
    pub is_crit: bool,
    pub skill_multiplier: f64,
    pub msg_id: u32,
}

#[derive(Debug, Clone)]
pub struct CombatDeathEvent {
    pub entity_id: u64,
    pub killer_id: u64,
    pub drops: Vec<CombatDrop>,
}

#[derive(Debug, Clone)]
pub struct CombatDrop {
    pub item_name: String,
    pub quantity: u32,
}

#[derive(Debug, Clone)]
pub struct CombatBuff {
    pub buff_type: String,
    pub value: i64,
    pub remaining_seconds: u32,
}

#[derive(Debug, Clone)]
pub struct CombatXpEvent {
    pub entity_id: u64,
    pub xp_gained: u64,
    pub leveled_up: bool,
}

impl CombatState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_entity(&mut self, id: u64, entity_type: &str, atk: i64, def: i64, crit_rate: f64, crit_dmg: f64, level: u32) {
        let hp = 100 * level as i64;
        self.entities.insert(id, CombatEntityState {
            entity_type: entity_type.to_string(),
            hp,
            max_hp: hp,
            atk,
            def,
            crit_rate,
            crit_dmg,
            level,
            xp: 0,
            xp_to_level: 100 * level as u64,
            kill_xp: 50 * level as u64,
            alive: true,
            xp_events_recorded: Vec::new(),
            level_ups: 0,
        });
    }

    pub fn set_hp(&mut self, id: u64, hp: i64) {
        if let Some(e) = self.entities.get_mut(&id) {
            e.hp = hp;
            e.max_hp = hp.max(e.max_hp);
        }
    }

    pub fn set_xp_to_level(&mut self, id: u64, remaining: u64) {
        if let Some(e) = self.entities.get_mut(&id) {
            let mut total = 0u64;
            for lvl in 0..e.level {
                total += 100 * lvl as u64;
            }
            e.xp = total + e.level as u64 * 100 - remaining;
            e.xp_to_level = e.level as u64 * 100;
        }
    }

    pub fn set_kill_xp(&mut self, id: u64, xp: u64) {
        if let Some(e) = self.entities.get_mut(&id) {
            e.kill_xp = xp;
        }
    }

    pub fn calculate_damage(&mut self, attacker_id: u64, target_id: u64, skill_multiplier: f64) -> i64 {
        let attacker = match self.entities.get(&attacker_id) {
            Some(a) => a.clone(),
            None => {
                self.last_damage = Some(0);
                return 0;
            }
        };
        let target = match self.entities.get(&target_id) {
            Some(t) => t.clone(),
            None => {
                self.last_damage = Some(0);
                return 0;
            }
        };

        // Apply buffs
        let mut atk = attacker.atk;
        let mut target_def = target.def;
        if let Some(buffs) = self.buffs.get(&attacker_id) {
            for b in buffs {
                if b.buff_type == "攻击加成" { atk += b.value; }
            }
        }
        if let Some(buffs) = self.buffs.get(&target_id) {
            for b in buffs {
                if b.buff_type == "防御降低" { target_def = (target_def - b.value).max(0); }
            }
        }

        // Base damage
        let raw = (atk as f64 * skill_multiplier) as i64;
        // Defense reduction
        let def_reduction = (target_def as f64 * 0.5) as i64;
        let mut damage = (raw - def_reduction).max(1);

        // Critical hit
        let is_crit = (rand::random::<f64>() * 100.0) < attacker.crit_rate;
        if is_crit {
            damage = (damage as f64 * attacker.crit_dmg) as i64;
        }

        // Apply damage
        if let Some(e) = self.entities.get_mut(&target_id) {
            e.hp = (e.hp - damage).max(0);
            if e.hp == 0 {
                e.alive = false;
            }
        }

        self.last_damage = Some(damage);
        self.last_action = Some(if is_crit { "暴击" } else { "普通攻击" }.to_string());

        let result = CombatResult {
            attacker_id,
            target_id,
            damage,
            is_crit,
            skill_multiplier,
            msg_id: 6001,
        };
        self.battle_results.push(result);

        // Register combat relation
        self.combat_relations.entry(attacker_id).or_default().insert(target_id);
        self.combat_relations.entry(target_id).or_default().insert(attacker_id);

        // Set broadcast targets
        self.broadcast_targets.push(attacker_id);
        self.broadcast_targets.push(target_id);

        // Handle death
        if !self.entities[&target_id].alive {
            // XP gain
            let kill_xp = self.entities[&target_id].kill_xp;
            if let Some(attacker) = self.entities.get_mut(&attacker_id) {
                attacker.xp += kill_xp;
                let leveled = attacker.xp >= attacker.xp_to_level;
                let xp_event = CombatXpEvent {
                    entity_id: attacker_id,
                    xp_gained: kill_xp,
                    leveled_up: leveled,
                };
                if leveled {
                    attacker.level += 1;
                    attacker.level_ups += 1;
                    attacker.xp_to_level = attacker.level as u64 * 100;
                    attacker.atk += 20;
                    attacker.def += 10;
                    attacker.max_hp += 50;
                    attacker.hp = attacker.max_hp;
                }
                attacker.xp_events_recorded.push(kill_xp as i64);
                self.xp_events.push(xp_event);
            }
            // Death event with drops
            let drops = vec![
                CombatDrop { item_name: "金币".to_string(), quantity: (target.level * 10) },
                CombatDrop { item_name: "经验药水".to_string(), quantity: 1 },
            ];
            self.death_events.push(CombatDeathEvent {
                entity_id: target_id,
                killer_id: attacker_id,
                drops,
            });
        }

        damage
    }

    pub fn calculate_aoe_damage(&mut self, attacker_id: u64, target_ids: &[u64], _range: f64) -> Vec<(u64, i64)> {
        let mut results = Vec::new();
        for &tid in target_ids {
            // AOE does 80% of single-target damage
            let dmg = self.calculate_damage(attacker_id, tid, 0.8);
            results.push((tid, dmg));
        }
        results
    }

    pub fn apply_buff(&mut self, target_id: u64, buff_type: &str, value: i64, duration_secs: u32) {
        self.buffs.entry(target_id).or_default().push(CombatBuff {
            buff_type: buff_type.to_string(),
            value,
            remaining_seconds: duration_secs,
        });
    }

    pub fn get_effective_atk(&self, id: u64) -> i64 {
        let base = self.entities.get(&id).map(|e| e.atk).unwrap_or(0);
        let bonus = self.buffs.get(&id).map(|buffs| {
            buffs.iter().filter(|b| b.buff_type == "攻击加成").map(|b| b.value).sum::<i64>()
        }).unwrap_or(0);
        base + bonus
    }

    pub fn get_effective_def(&self, id: u64) -> i64 {
        let base = self.entities.get(&id).map(|e| e.def).unwrap_or(0);
        let penalty = self.buffs.get(&id).map(|buffs| {
            buffs.iter().filter(|b| b.buff_type == "防御降低").map(|b| b.value).sum::<i64>()
        }).unwrap_or(0);
        (base - penalty).max(0)
    }

    pub fn set_combat_state(&mut self, id: u64, state: &str) {
        self.combat_states.insert(id, state.to_string());
    }

    pub fn get_combat_state(&self, id: u64) -> String {
        self.combat_states.get(&id).cloned().unwrap_or_else(|| "空闲".to_string())
    }

    pub fn is_dead(&self, id: u64) -> bool {
        self.entities.get(&id).map(|e| !e.alive).unwrap_or(false)
    }

    pub fn get_hp(&self, id: u64) -> i64 {
        self.entities.get(&id).map(|e| e.hp).unwrap_or(0)
    }
}

/// 聊天服模拟状态（BDD 测试用）
#[derive(Debug, Default)]
pub struct ChatState {
    pub channels: std::collections::HashMap<String, ChatChannel>,
    pub connected_players: std::collections::HashSet<u64>,
    pub player_channels: std::collections::HashMap<u64, Vec<String>>,
    pub guilds: std::collections::HashMap<String, std::collections::HashSet<u64>>,
    pub parties: std::collections::HashMap<u64, std::collections::HashSet<u64>>,
    pub private_messages: Vec<(u64, u64, String)>,
    pub guild_messages: Vec<(u64, String, String)>,
    pub party_messages: Vec<(u64, u64, String)>,
    pub broadcast_receivers: std::collections::HashMap<String, std::collections::HashSet<u64>>,
    pub history_results: Vec<String>,
    pub rate_limit_counters: std::collections::HashMap<u64, u32>,
    pub max_messages_per_second: u32,
    pub max_message_length: usize,
    pub sensitive_words: Vec<String>,
    pub filtered_event: bool,
    pub offline_messages: std::collections::HashMap<u64, Vec<String>>,
    pub rate_limit_warnings: Vec<u64>,
    pub message_too_long_errors: Vec<u64>,
    pub last_error: Option<String>,
    pub acks: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct ChatChannel {
    pub name: String,
    pub channel_type: String,
    pub messages: Vec<(u64, String)>,
    pub members: std::collections::HashSet<u64>,
}

impl ChatState {
    pub fn new() -> Self {
        Self { max_messages_per_second: 5, max_message_length: 500, ..Default::default() }
    }

    pub fn connect_player(&mut self, uid: u64) {
        self.connected_players.insert(uid);
    }

    pub fn disconnect_player(&mut self, uid: u64) {
        self.connected_players.remove(&uid);
    }

    pub fn ensure_channel(&mut self, name: &str, channel_type: &str) {
        self.channels.entry(name.to_string()).or_insert_with(|| ChatChannel {
            name: name.to_string(),
            channel_type: channel_type.to_string(),
            messages: Vec::new(),
            members: std::collections::HashSet::new(),
        });
    }

    pub fn join_channel(&mut self, uid: u64, name: &str) {
        self.ensure_channel(name, "world");
        if let Some(ch) = self.channels.get_mut(name) {
            ch.members.insert(uid);
        }
        self.player_channels.entry(uid).or_default().push(name.to_string());
    }

    pub fn send_message(&mut self, uid: u64, channel: &str, text: &str) -> Result<(), String> {
        if text.len() > self.max_message_length {
            self.message_too_long_errors.push(uid);
            return Err("消息过长".to_string());
        }
        for word in &self.sensitive_words {
            if text.contains(word) {
                self.filtered_event = true;
                return Err("消息被过滤".to_string());
            }
        }
        let count = self.rate_limit_counters.entry(uid).or_insert(0);
        if *count >= self.max_messages_per_second {
            self.rate_limit_warnings.push(uid);
            return Err("发送频率过快".to_string());
        }
        *count += 1;

        self.ensure_channel(channel, "world");
        if let Some(ch) = self.channels.get_mut(channel) {
            ch.messages.push((uid, text.to_string()));
        }
        self.acks.push(uid);

        if let Some(ch) = self.channels.get(channel) {
            for &member in &ch.members {
                if member != uid {
                    self.broadcast_receivers.entry(channel.to_string()).or_default().insert(member);
                }
            }
        }
        Ok(())
    }

    pub fn send_private(&mut self, from: u64, to: u64, text: &str) -> Result<(), String> {
        if text.len() > self.max_message_length {
            self.message_too_long_errors.push(from);
            return Err("消息过长".to_string());
        }
        for word in &self.sensitive_words {
            if text.contains(word) {
                self.filtered_event = true;
                return Err("消息被过滤".to_string());
            }
        }
        let count = self.rate_limit_counters.entry(from).or_insert(0);
        if *count >= self.max_messages_per_second {
            self.rate_limit_warnings.push(from);
            return Err("发送频率过快".to_string());
        }
        *count += 1;

        self.private_messages.push((from, to, text.to_string()));
        self.acks.push(from);

        if !self.connected_players.contains(&to) {
            self.offline_messages.entry(to).or_default().push(text.to_string());
        }
        Ok(())
    }

    pub fn send_guild(&mut self, from: u64, guild: &str, text: &str) -> Result<(), String> {
        if text.len() > self.max_message_length {
            self.message_too_long_errors.push(from);
            return Err("消息过长".to_string());
        }
        for word in &self.sensitive_words {
            if text.contains(word) {
                self.filtered_event = true;
                return Err("消息被过滤".to_string());
            }
        }
        let count = self.rate_limit_counters.entry(from).or_insert(0);
        if *count >= self.max_messages_per_second {
            self.rate_limit_warnings.push(from);
            return Err("发送频率过快".to_string());
        }
        *count += 1;

        self.guild_messages.push((from, guild.to_string(), text.to_string()));
        self.acks.push(from);

        if let Some(members) = self.guilds.get(guild) {
            for &member in members {
                if member != from {
                    let ch_key = format!("guild:{}", guild);
                    self.broadcast_receivers.entry(ch_key).or_default().insert(member);
                }
            }
        }
        Ok(())
    }

    pub fn send_party(&mut self, from: u64, party_id: u64, text: &str) -> Result<(), String> {
        if text.len() > self.max_message_length {
            self.message_too_long_errors.push(from);
            return Err("消息过长".to_string());
        }
        for word in &self.sensitive_words {
            if text.contains(word) {
                self.filtered_event = true;
                return Err("消息被过滤".to_string());
            }
        }
        let count = self.rate_limit_counters.entry(from).or_insert(0);
        if *count >= self.max_messages_per_second {
            self.rate_limit_warnings.push(from);
            return Err("发送频率过快".to_string());
        }
        *count += 1;

        self.party_messages.push((from, party_id, text.to_string()));
        self.acks.push(from);

        if let Some(members) = self.parties.get(&party_id) {
            for &member in members {
                if member != from {
                    let ch_key = format!("party:{}", party_id);
                    self.broadcast_receivers.entry(ch_key).or_default().insert(member);
                }
            }
        }
        Ok(())
    }

    pub fn query_history(&mut self, channel: &str, limit: usize) -> Vec<String> {
        self.ensure_channel(channel, "world");
        let ch = self.channels.get(channel).unwrap();
        let msgs: Vec<String> = ch.messages.iter().map(|(_, t)| t.clone()).collect();
        let start = if msgs.len() > limit { msgs.len() - limit } else { 0 };
        let result = msgs[start..].to_vec();
        self.history_results = result.clone();
        result
    }

    pub fn set_sensitive_words(&mut self, words: &[&str]) {
        self.sensitive_words = words.iter().map(|s| s.to_string()).collect();
    }

    pub fn create_guild(&mut self, name: &str) {
        self.guilds.entry(name.to_string()).or_default();
    }

    pub fn join_guild(&mut self, uid: u64, name: &str) {
        self.guilds.entry(name.to_string()).or_default().insert(uid);
    }

    pub fn create_party(&mut self, party_id: u64) {
        self.parties.entry(party_id).or_default();
    }

    pub fn join_party(&mut self, uid: u64, party_id: u64) {
        self.parties.entry(party_id).or_default().insert(uid);
    }
}

use cucumber::World;

#[derive(World)]
#[world(init = Self::new)]
pub struct BddWorld {
    // ── 连接与握手状态 ──
    pub tcp_connected: bool,
    pub handshake_stage: bool,
    pub connection_rejected: bool,
    pub reject_reason: Option<String>,
    pub security_log_count: u32,

    // ── 会话状态 ──
    pub sessions: std::collections::HashMap<String, TestSession>,
    pub session_id_counter: u64,
    pub kicked_sessions: Vec<String>,
    pub zombie_cleaned: Vec<String>,

    // ── 协议状态 ──
    pub encoder: Option<PacketEncoder>,
    pub decoder: Option<PacketDecoder>,
    pub last_decoded_payload: Option<Vec<u8>>,
    pub last_encoded_bytes: Option<Vec<u8>>,
    pub decode_error: Option<String>,
    pub connection_disconnected: bool,

    // ── 消息状态 ──
    pub priority_queue: PriorityQueue,
    pub packet_merge: PacketMerge,
    pub merged_packet_count: usize,
    pub dropped_packets: u32,
    pub routed_messages: Vec<(u64, u16)>, // (uid, msg_id)
    pub delivered_messages: Vec<(u64, u16)>,

    // ── 安全状态 ──
    pub rate_limiter: Option<RateLimiter>,
    pub ip_blacklist: Option<IpBlacklist>,
    pub rate_limited: bool,
    pub audit_events: Vec<String>,
    pub auto_blocked_ips: Vec<String>,

    // ── 集群状态 ──
    pub registered_nodes: std::collections::HashMap<String, NodeInfo>,
    pub heartbeat_count: u32,
    pub removed_nodes: Vec<String>,
    pub pubsub_messages: Vec<(String, String, u64)>, // (from_gate, to_gate, uid)
    pub route_map: std::collections::HashMap<u64, String>, // uid -> gate_name

    // ── 停机状态 ──
    pub accepting_new_connections: bool,
    pub shutdown_started: bool,
    pub shutdown_complete: bool,
    pub notified_logic_server_count: u32,
    pub startup_ready: bool,

    // ── 场景服状态 ──
    pub scene_state: Option<SceneState>,

    // ── 战斗服状态 ──
    pub combat_state: Option<CombatState>,

    // ── 聊天服状态 ──
    pub chat_state: Option<ChatState>,

    // ── 通用 ──
    pub elapsed_seconds: u64,
}

/// 测试用会话（不需要真实 TCP 连接）
#[derive(Clone, Debug)]
pub struct TestSession {
    pub session_id: String,
    pub player_uid: u64,
    pub state: SessionState,
    pub last_active_secs_ago: u64,
    pub closed: bool,
}

/// 集群节点信息
#[derive(Clone, Debug)]
pub struct NodeInfo {
    pub node_id: u64,
    pub node_name: String,
    pub address: String,
    pub online_count: usize,
    pub last_heartbeat_secs_ago: u64,
    pub alive: bool,
}

impl BddWorld {
    fn new() -> Self {
        Self {
            tcp_connected: false,
            handshake_stage: false,
            connection_rejected: false,
            reject_reason: None,
            security_log_count: 0,

            sessions: std::collections::HashMap::new(),
            session_id_counter: 0,
            kicked_sessions: Vec::new(),
            zombie_cleaned: Vec::new(),

            encoder: None,
            decoder: None,
            last_decoded_payload: None,
            last_encoded_bytes: None,
            decode_error: None,
            connection_disconnected: false,

            priority_queue: PriorityQueue::new(),
            packet_merge: PacketMerge::new(std::time::Duration::from_millis(16), AesGcmCipher::from_hex_key("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff").unwrap()),
            merged_packet_count: 0,
            dropped_packets: 0,
            routed_messages: Vec::new(),
            delivered_messages: Vec::new(),

            rate_limiter: None,
            ip_blacklist: None,
            rate_limited: false,
            audit_events: Vec::new(),
            auto_blocked_ips: Vec::new(),

            registered_nodes: std::collections::HashMap::new(),
            heartbeat_count: 0,
            removed_nodes: Vec::new(),
            pubsub_messages: Vec::new(),
            route_map: std::collections::HashMap::new(),

            accepting_new_connections: true,
            shutdown_started: false,
            shutdown_complete: false,
            notified_logic_server_count: 0,
            startup_ready: false,

            scene_state: None,

            combat_state: None,

            chat_state: None,

            elapsed_seconds: 0,
        }
    }

    /// 辅助：记录安全日志
    pub fn log_security(&mut self) {
        self.security_log_count += 1;
    }

    /// 辅助：断开连接
    pub fn disconnect(&mut self) {
        self.connection_disconnected = true;
        self.tcp_connected = false;
    }

    /// 辅助：初始化编解码器
    pub fn init_codec(&mut self) {
        let key = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let cipher = AesGcmCipher::from_hex_key(key).unwrap();
        self.encoder = Some(PacketEncoder::new(cipher));
        let cipher2 = AesGcmCipher::from_hex_key(key).unwrap();
        self.decoder = Some(PacketDecoder::new(cipher2));
    }

    /// 辅助：创建测试会话
    pub fn create_test_session(&mut self, uid: u64) -> String {
        self.session_id_counter += 1;
        let sid = format!("session-{}", self.session_id_counter);
        self.sessions.insert(
            sid.clone(),
            TestSession {
                session_id: sid.clone(),
                player_uid: uid,
                state: SessionState::Online,
                last_active_secs_ago: 0,
                closed: false,
            },
        );
        sid
    }
}

// 手动实现 Debug（部分字段类型未实现 Debug）
impl std::fmt::Debug for BddWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BddWorld")
            .field("tcp_connected", &self.tcp_connected)
            .field("handshake_stage", &self.handshake_stage)
            .field("connection_rejected", &self.connection_rejected)
            .field("security_log_count", &self.security_log_count)
            .field("session_count", &self.sessions.len())
            .field("kicked_sessions", &self.kicked_sessions)
            .field("connection_disconnected", &self.connection_disconnected)
            .field("merged_packet_count", &self.merged_packet_count)
            .field("dropped_packets", &self.dropped_packets)
            .field("rate_limited", &self.rate_limited)
            .field("registered_nodes", &self.registered_nodes.len())
            .field("heartbeat_count", &self.heartbeat_count)
            .field("accepting_new_connections", &self.accepting_new_connections)
            .field("shutdown_complete", &self.shutdown_complete)
            .field("startup_ready", &self.startup_ready)
            .field("scene_state", &self.scene_state.is_some())
            .field("combat_state", &self.combat_state.is_some())
            .field("chat_state", &self.chat_state.is_some())
            .finish()
    }
}

// 引入需要的类型
use rust_mmo_gate::crypto::aes_gcm::AesGcmCipher;
use rust_mmo_gate::io_engine::msg_priority::PriorityQueue;
use rust_mmo_gate::io_engine::packet_merge::PacketMerge;
use rust_mmo_gate::protocol::decoder::PacketDecoder;
use rust_mmo_gate::protocol::encoder::PacketEncoder;
use rust_mmo_gate::security::ip_blacklist::IpBlacklist;
use rust_mmo_gate::security::rate_limit::RateLimiter;
use rust_mmo_gate::session::session_struct::SessionState;

#[tokio::main]
async fn main() {
    BddWorld::cucumber()
        .run_and_exit("tests/bdd_feature")
        .await;
}
