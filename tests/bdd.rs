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
