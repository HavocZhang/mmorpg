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
                let was_in_range = old_pos.map_or(false, |(px, py)| {
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
