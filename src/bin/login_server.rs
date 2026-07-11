//! 登录服 (Login Server)
//!
//! MMORPG 认证与角色管理微服务。
//! 作为 gRPC LogicService 后端，通过网关接收客户端消息。
//!
//! ## 功能
//! 1. 账号登录（用户名+密码 → JWT Token + UID）
//! 2. 账号注册
//! 3. 角色列表查询
//! 4. 创建角色（选职业）
//! 5. 选择角色进入世界
//!
//! ## 消息协议
//!
//! **上行 (Client → Gateway → LoginServer):**
//! ```
//! 101: 登录      {"username":"xxx","password":"xxx"}
//! 102: 注册      {"username":"xxx","password":"xxx"}
//! 103: 角色列表   {}
//! 104: 创建角色   {"name":"Hero","class":"warrior"}
//! 105: 选择角色   {"charId":1}
//! ```
//!
//! **下行 (LoginServer → Gateway → Client):**
//! ```
//! 5101: 登录结果      {"success":true,"uid":10001,"token":"xxx","username":"test"}
//! 5102: 注册结果      {"success":true,"uid":10002,"username":"newuser"}
//! 5103: 角色列表      {"chars":[{...}]}
//! 5104: 创建角色结果   {"success":true,"char":{...}}
//! 5105: 进入世界      {"success":true,"worldToken":"xxx","char":{...}}
//! ```
//!
//! ## 运行方式
//! ```bash
//! cargo run --bin login-server
//! ```
//!
//! ## 架构位置
//! ```
//! Client → Gateway (:7888) → gRPC → LoginServer (:50052)
//!                                     ↓
//!                               [内存存储: Users + Characters]
//! ```

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tonic::{transport::Server, Request, Response, Status};
use uuid::Uuid;

use rust_mmo_gate::grpc_router::proto::gate::{
    logic_service_server::{LogicService, LogicServiceServer},
    DownstreamMessage, ForwardBatchRequest, ForwardRequest, ForwardResponse,
    PlayerOfflineRequest, PlayerOfflineResponse, PlayerOnlineRequest, PlayerOnlineResponse,
};

// ════════════════════════════════════════════════════════════════
// 常量
// ════════════════════════════════════════════════════════════════

/// JWT 密钥（生产环境应从环境变量读取）
const JWT_SECRET: &str = "mmo-login-secret-key-2026";

/// Token 有效期（秒）
const TOKEN_TTL_SECS: u64 = 7200; // 2 小时

/// 世界 Token 有效期（秒）
const WORLD_TOKEN_TTL_SECS: u64 = 300; // 5 分钟

/// 可选职业
const CLASSES: &[(&str, &str)] = &[
    ("warrior", "战士"),
    ("mage", "法师"),
    ("archer", "弓箭手"),
    ("priest", "牧师"),
    ("rogue", "刺客"),
];

/// 每个账号最大角色数
const MAX_CHARS_PER_ACCOUNT: usize = 4;

/// 角色名最大长度（中文字符）
const MAX_CHAR_NAME_LEN: usize = 12;

// ════════════════════════════════════════════════════════════════
// 数据结构
// ════════════════════════════════════════════════════════════════

/// 用户账户
#[derive(Debug, Clone, Serialize, Deserialize)]
struct User {
    uid: u64,
    username: String,
    password_hash: String,
    created_at: u64,
    last_login: u64,
}

/// 角色
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Character {
    id: u64,
    uid: u64,
    name: String,
    class: String,
    level: u32,
    created_at: u64,
}

/// JWT Claims
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,       // uid as string
    username: String,
    exp: usize,        // expiration timestamp
    iat: usize,        // issued at
    jti: String,       // unique token id
}

// ════════════════════════════════════════════════════════════════
// 登录服务实现
// ════════════════════════════════════════════════════════════════

pub struct LoginService {
    /// username -> User
    users: DashMap<String, User>,
    /// uid -> User (快速查找)
    users_by_uid: DashMap<u64, User>,
    /// token -> uid (有效会话)
    tokens: DashMap<String, u64>,
    /// uid -> Vec<Character>
    characters: DashMap<u64, Vec<Character>>,
    /// char_id -> Character
    characters_by_id: DashMap<u64, Character>,
    /// 自增 UID
    next_uid: AtomicU64,
    /// 自增角色ID
    next_char_id: AtomicU64,
}

impl LoginService {
    fn new() -> Self {
        let service = Self {
            users: DashMap::new(),
            users_by_uid: DashMap::new(),
            tokens: DashMap::new(),
            characters: DashMap::new(),
            characters_by_id: DashMap::new(),
            next_uid: AtomicU64::new(10001),
            next_char_id: AtomicU64::new(1),
        };

        // 预创建测试账号
        service.create_test_accounts();

        println!("[LoginServer] 初始化完成，在线用户: {}", service.users.len());
        service
    }

    /// 创建测试账号（开发用）
    fn create_test_accounts(&self) {
        let test_accounts = [
            ("test", "123456"),
            ("admin", "admin123"),
            ("player1", "111111"),
            ("player2", "222222"),
            ("player3", "333333"),
        ];

        for (username, password) in &test_accounts {
            let hash = bcrypt::hash(password, bcrypt::DEFAULT_COST)
                .unwrap_or_else(|_| format!("bcrypt_failed_{}", username));
            let uid = self.next_uid.fetch_add(1, Ordering::Relaxed);
            let user = User {
                uid,
                username: username.to_string(),
                password_hash: hash,
                created_at: now_secs(),
                last_login: 0,
            };
            self.users.insert(username.to_string(), user.clone());
            self.users_by_uid.insert(uid, user);
            self.characters.insert(uid, Vec::new());
            println!("[LoginServer] 预创建账号: {} (uid={})", username, uid);
        }
    }

    /// 生成 JWT Token
    fn generate_token(&self, uid: u64, username: &str, ttl: u64) -> Result<String, String> {
        let now = now_secs() as usize;
        let claims = Claims {
            sub: uid.to_string(),
            username: username.to_string(),
            exp: now + ttl as usize,
            iat: now,
            jti: Uuid::new_v4().to_string(),
        };

        let header = jsonwebtoken::Header::default();
        let key = jsonwebtoken::EncodingKey::from_secret(JWT_SECRET.as_bytes());
        jsonwebtoken::encode(&header, &claims, &key).map_err(|e| format!("JWT encode: {}", e))
    }

    /// 验证 JWT Token
    fn verify_token(&self, token: &str) -> Result<Claims, String> {
        let key = jsonwebtoken::DecodingKey::from_secret(JWT_SECRET.as_bytes());
        let validation = jsonwebtoken::Validation::default();
        jsonwebtoken::decode::<Claims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(|e| format!("JWT verify: {}", e))
    }
}

impl Default for LoginService {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl LogicService for LoginService {
    async fn forward_message(
        &self,
        request: Request<ForwardRequest>,
    ) -> Result<Response<ForwardResponse>, Status> {
        let req = request.into_inner();
        let response = self.process_message(req.player_uid, req.msg_id, &req.payload);
        Ok(Response::new(response))
    }

    async fn forward_message_batch(
        &self,
        request: Request<ForwardBatchRequest>,
    ) -> Result<Response<ForwardResponse>, Status> {
        let req = request.into_inner();
        let mut all_messages = Vec::new();
        for msg in req.messages {
            let resp = self.process_message(msg.player_uid, msg.msg_id, &msg.payload);
            all_messages.extend(resp.messages);
        }
        Ok(Response::new(ForwardResponse {
            messages: all_messages,
        }))
    }

    async fn player_online(
        &self,
        request: Request<PlayerOnlineRequest>,
    ) -> Result<Response<PlayerOnlineResponse>, Status> {
        let req = request.into_inner();
        println!(
            "[LoginServer] 玩家上线通知: uid={} session={} gate={}",
            req.player_uid, req.session_id, req.gate_node
        );

        // 登录服不参与游戏内逻辑，仅记录上线事件
        Ok(Response::new(PlayerOnlineResponse {
            ok: true,
            messages: vec![],
        }))
    }

    async fn player_offline(
        &self,
        request: Request<PlayerOfflineRequest>,
    ) -> Result<Response<PlayerOfflineResponse>, Status> {
        let req = request.into_inner();
        println!(
            "[LoginServer] 玩家离线通知: uid={} reason={}",
            req.player_uid, req.reason
        );

        Ok(Response::new(PlayerOfflineResponse {
            ok: true,
            messages: vec![],
        }))
    }
}

impl LoginService {
    /// 消息路由：根据 msg_id 分发到不同处理函数
    fn process_message(&self, uid: u64, msg_id: u32, payload: &[u8]) -> ForwardResponse {
        let payload_str = String::from_utf8_lossy(payload);
        let json: Value = serde_json::from_str(&payload_str).unwrap_or(Value::Null);

        let messages = match msg_id {
            101 => self.handle_login(uid, &json),
            102 => self.handle_register(uid, &json),
            103 => self.handle_char_list(uid),
            104 => self.handle_create_char(uid, &json),
            105 => self.handle_select_char(uid, &json),
            106 => self.handle_token_verify(uid, &json), // 网关 token 校验
            _ => {
                let echo = serde_json::json!({
                    "type": "echo",
                    "server": "login",
                    "msg_id": msg_id,
                    "data": &payload_str,
                })
                .to_string();
                vec![dm(uid, msg_id + 5000, echo, 0)]
            }
        };

        ForwardResponse { messages }
    }

    // ════════════════════════════════════════════════════════════
    // 101: 登录
    // ════════════════════════════════════════════════════════════
    fn handle_login(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let username = json
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let password = json
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if username.is_empty() || password.is_empty() {
            return vec![err_msg(uid, 5101, "请输入用户名和密码")];
        }

        let user = match self.users.get(&username) {
            Some(u) => u.clone(),
            None => {
                return vec![err_msg(uid, 5101, "用户名或密码错误")];
            }
        };

        // 验证密码
        match bcrypt::verify(&password, &user.password_hash) {
            Ok(true) => {}
            _ => {
                return vec![err_msg(uid, 5101, "用户名或密码错误")];
            }
        }

        // 生成 JWT Token
        let token = match self.generate_token(user.uid, &user.username, TOKEN_TTL_SECS) {
            Ok(t) => t,
            Err(e) => {
                return vec![err_msg(uid, 5101, &format!("Token 生成失败: {}", e))];
            }
        };

        // 保存 token（用于快速校验）
        self.tokens.insert(token.clone(), user.uid);

        // 更新最后登录时间
        if let Some(mut u) = self.users.get_mut(&username) {
            u.last_login = now_secs();
        }

        println!(
            "[LoginServer] 登录成功: {} (uid={}) tokens_active={}",
            username,
            user.uid,
            self.tokens.len()
        );

        // 获取角色列表
        let chars = self
            .characters
            .get(&user.uid)
            .map(|c| c.clone())
            .unwrap_or_default();

        let chars_json: Vec<Value> = chars
            .iter()
            .map(char_to_json_value)
            .collect();

        let result = serde_json::json!({
            "success": true,
            "uid": user.uid,
            "username": user.username,
            "token": token,
            "tokenExpiresIn": TOKEN_TTL_SECS,
            "chars": chars_json,
            "maxChars": MAX_CHARS_PER_ACCOUNT,
            "availableClasses": CLASSES.iter().map(|(id, name)| {
                serde_json::json!({"id": id, "name": name})
            }).collect::<Vec<_>>(),
        })
        .to_string();

        vec![dm(user.uid, 5101, result, 2)]
    }

    // ════════════════════════════════════════════════════════════
    // 102: 注册
    // ════════════════════════════════════════════════════════════
    fn handle_register(&self, req_uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let username = json
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let password = json
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 验证用户名
        if username.len() < 3 || username.len() > 20 {
            return vec![err_msg(req_uid, 5102, "用户名长度需在 3-20 个字符之间")];
        }

        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return vec![err_msg(req_uid, 5102, "用户名只能包含字母、数字和下划线")];
        }

        // 验证密码
        if password.len() < 6 {
            return vec![err_msg(req_uid, 5102, "密码长度至少 6 个字符")];
        }

        // 检查用户名是否已存在
        if self.users.contains_key(&username) {
            return vec![err_msg(req_uid, 5102, "用户名已被注册")];
        }

        // 哈希密码
        let hash = match bcrypt::hash(&password, bcrypt::DEFAULT_COST) {
            Ok(h) => h,
            Err(_) => {
                return vec![err_msg(req_uid, 5102, "服务器内部错误")];
            }
        };

        let uid = self.next_uid.fetch_add(1, Ordering::Relaxed);
        let now = now_secs();

        let user = User {
            uid,
            username: username.clone(),
            password_hash: hash,
            created_at: now,
            last_login: now,
        };

        self.users.insert(username.clone(), user.clone());
        self.users_by_uid.insert(uid, user);
        self.characters.insert(uid, Vec::new());

        // 生成 Token
        let token = self.generate_token(uid, &username, TOKEN_TTL_SECS)
            .unwrap_or_else(|_| format!("tok_{}", Uuid::new_v4()));

        self.tokens.insert(token.clone(), uid);

        println!(
            "[LoginServer] 注册成功: {} (uid={}) total_users={}",
            username,
            uid,
            self.users.len()
        );

        let result = serde_json::json!({
            "success": true,
            "uid": uid,
            "username": username,
            "token": token,
            "tokenExpiresIn": TOKEN_TTL_SECS,
            "chars": [],
            "maxChars": MAX_CHARS_PER_ACCOUNT,
            "availableClasses": CLASSES.iter().map(|(id, name)| {
                serde_json::json!({"id": id, "name": name})
            }).collect::<Vec<_>>(),
        })
        .to_string();

        vec![dm(uid, 5102, result, 2)]
    }

    // ════════════════════════════════════════════════════════════
    // 103: 角色列表
    // ════════════════════════════════════════════════════════════
    fn handle_char_list(&self, uid: u64) -> Vec<DownstreamMessage> {
        if uid == 0 {
            return vec![err_msg(uid, 5103, "请先登录")];
        }

        let chars = self
            .characters
            .get(&uid)
            .map(|c| c.clone())
            .unwrap_or_default();

        let chars_json: Vec<Value> = chars.iter().map(char_to_json_value).collect();

        let result = serde_json::json!({
            "success": true,
            "uid": uid,
            "chars": chars_json,
            "maxChars": MAX_CHARS_PER_ACCOUNT,
            "availableClasses": CLASSES.iter().map(|(id, name)| {
                serde_json::json!({"id": id, "name": name})
            }).collect::<Vec<_>>(),
        })
        .to_string();

        vec![dm(uid, 5103, result, 1)]
    }

    // ════════════════════════════════════════════════════════════
    // 104: 创建角色
    // ════════════════════════════════════════════════════════════
    fn handle_create_char(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        if uid == 0 {
            return vec![err_msg(uid, 5104, "请先登录")];
        }

        let name = json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let class = json
            .get("class")
            .and_then(|v| v.as_str())
            .unwrap_or("warrior")
            .to_string();

        // 验证角色名
        if name.is_empty() {
            return vec![err_msg(uid, 5104, "请输入角色名")];
        }

        // 角色名长度校验（UTF-8 字节数）
        if name.len() > MAX_CHAR_NAME_LEN * 3 {
            // 粗略估计中文字符
            let msg = format!("角色名不能超过{}个字符", MAX_CHAR_NAME_LEN); return vec![err_msg(uid, 5104, &msg)];
        }

        // 验证职业
        if !CLASSES.iter().any(|(id, _)| *id == class) {
            let valid: Vec<&str> = CLASSES.iter().map(|(id, _)| *id).collect();
            let msg = format!("无效职业，可选: {:?}", valid); return vec![err_msg(uid, 5104, &msg)];
        }

        // 检查角色数量上限
        let char_count = self
            .characters
            .get(&uid)
            .map(|c| c.len())
            .unwrap_or(0);

        if char_count >= MAX_CHARS_PER_ACCOUNT {
            let msg = format!("最多创建 {} 个角色", MAX_CHARS_PER_ACCOUNT);
            return vec![err_msg(uid, 5104, &msg)];
        }

        // 检查角色名唯一性
        for chars in self.characters.iter() {
            if chars.iter().any(|c| c.name == name) {
                return vec![err_msg(uid, 5104, "角色名已被使用")];
            }
        }

        let char_id = self.next_char_id.fetch_add(1, Ordering::Relaxed);
        let character = Character {
            id: char_id,
            uid,
            name: name.clone(),
            class: class.clone(),
            level: 1,
            created_at: now_secs(),
        };

        // 存储角色
        self.characters_by_id.insert(char_id, character.clone());

        let mut chars = self.characters.get(&uid).map(|c| c.clone()).unwrap_or_default();
        chars.push(character.clone());
        self.characters.insert(uid, chars);

        println!(
            "[LoginServer] 角色创建: uid={} char={} class={} charId={}",
            uid, name, class, char_id
        );

        let result = serde_json::json!({
            "success": true,
            "char": char_to_json_value(&character),
        })
        .to_string();

        vec![dm(uid, 5104, result, 2)]
    }

    // ════════════════════════════════════════════════════════════
    // 105: 选择角色进入世界
    // ════════════════════════════════════════════════════════════
    fn handle_select_char(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        if uid == 0 {
            return vec![err_msg(uid, 5105, "请先登录")];
        }

        let char_id = json
            .get("charId")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if char_id == 0 {
            return vec![err_msg(uid, 5105, "请选择角色")];
        }

        let character = match self.characters_by_id.get(&char_id) {
            Some(c) => c.clone(),
            None => {
                return vec![err_msg(uid, 5105, "角色不存在")];
            }
        };

        // 验证角色归属
        if character.uid != uid {
            return vec![err_msg(uid, 5105, "无权操作此角色")];
        }

        // 生成世界 Token（短时效，用于进入场景服）
        let world_token = self
            .generate_token(uid, &format!("char_{}", char_id), WORLD_TOKEN_TTL_SECS)
            .unwrap_or_else(|_| format!("wtok_{}", Uuid::new_v4()));

        println!(
            "[LoginServer] 进入世界: uid={} char={}(id={}) class={}",
            uid, character.name, char_id, character.class
        );

        let result = serde_json::json!({
            "success": true,
            "char": char_to_json_value(&character),
            "worldToken": world_token,
            "worldTokenExpiresIn": WORLD_TOKEN_TTL_SECS,
            "gatewayHost": "127.0.0.1",
            "gatewayPort": 7888,
            "sceneServer": "grpc://127.0.0.1:50051",
        })
        .to_string();

        vec![dm(uid, 5105, result, 2)]
    }

    // ════════════════════════════════════════════════════════════
    // 106: Token 验证（供网关调用）
    // ════════════════════════════════════════════════════════════
    fn handle_token_verify(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let token = json
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 先查内存缓存
        if let Some(uid) = self.tokens.get(&token) {
            let result = serde_json::json!({
                "valid": true,
                "uid": *uid,
                "source": "cache",
            })
            .to_string();
            // token_verify 通常由网关在握手时调用，回给网关处理
            // 这里 uid 使用 0 因为调用方是网关自身
            return vec![dm(0, 5106, result, 1)];
        }

        // JWT 本地验证
        match self.verify_token(&token) {
            Ok(claims) => {
                let uid: u64 = claims.sub.parse().unwrap_or(0);
                // 缓存到内存
                self.tokens.insert(token.clone(), uid);
                let result = serde_json::json!({
                    "valid": true,
                    "uid": uid,
                    "username": claims.username,
                    "source": "jwt",
                })
                .to_string();
                vec![dm(0, 5106, result, 1)]
            }
            Err(e) => {
                vec![err_msg(uid, 5106, &format!("Token 无效: {}", e))]
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════
// 辅助函数
// ════════════════════════════════════════════════════════════════

/// 构造一个下行消息
fn dm(target_uid: u64, msg_id: u32, payload: String, priority: u32) -> DownstreamMessage {
    DownstreamMessage {
        target_uid,
        msg_id,
        payload: payload.into_bytes(),
        priority,
    }
}

/// 构造一个错误响应（发给指定玩家）
fn err_msg(uid: u64, msg_id: u32, error: &str) -> DownstreamMessage {
    let json = serde_json::json!({
        "success": false,
        "error": error,
    })
    .to_string();
    DownstreamMessage {
        target_uid: uid,
        msg_id,
        payload: json.into_bytes(),
        priority: 2,
    }
}

/// 将角色转为 JSON Value
fn char_to_json_value(c: &Character) -> Value {
    serde_json::json!({
        "id": c.id,
        "uid": c.uid,
        "name": c.name,
        "class": c.class,
        "className": class_display_name(&c.class),
        "level": c.level,
        "createdAt": c.created_at,
    })
}

/// 获取职业中文名
fn class_display_name(class_id: &str) -> &str {
    CLASSES
        .iter()
        .find(|(id, _)| *id == class_id)
        .map(|(_, name)| *name)
        .unwrap_or("未知")
}

/// 当前 Unix 时间戳（秒）
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ════════════════════════════════════════════════════════════════
// Main
// ════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "0.0.0.0:50052".parse()?;

    println!("╔═══════════════════════════════════════════╗");
    println!("║   MMORPG 登录服 (Login Server)            ║");
    println!("╠═══════════════════════════════════════════╣");
    println!("║   gRPC 监听: {}                    ║", addr);
    println!("║   Token 算法: JWT (HS256)                ║");
    println!("║   密码哈希: bcrypt (cost=12)             ║");
    println!("║   存储: 内存 (DashMap)                    ║");
    println!("║   测试账号: test/123456                   ║");
    println!("╚═══════════════════════════════════════════╝");
    println!();
    println!("消息协议:");
    println!("  上行: 101=登录 102=注册 103=角色列表 104=创建角色 105=选择角色 106=Token验证");
    println!("  下行: 5101=登录结果 5102=注册结果 5103=角色列表 5104=创建结果 5105=进入世界 5106=Token验证");
    println!("  可选职业: {}", CLASSES.iter().map(|(id, name)| format!("{}({})", id, name)).collect::<Vec<_>>().join(", "));
    println!();

    let service = LoginService::new();

    println!("[LoginServer] 启动完成，等待网关连接...");
    println!();

    Server::builder()
        .add_service(LogicServiceServer::new(service))
        .serve(addr)
        .await?;

    println!("[LoginServer] 已停止");
    Ok(())
}
