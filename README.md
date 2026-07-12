# Rust MMO Gate — MMORPG 从零到一实战教程

[![Rust](https://img.shields.io/badge/Rust-1.95.0-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://img.shields.io/badge/CI-passing-brightgreen.svg)](ci.sh)
[![Security](https://img.shields.io/badge/audit-0%20vulns-brightgreen.svg)](#安全审计)
[![Throughput](https://img.shields.io/badge/throughput-80K%20pps-blue.svg)](#吞吐压测)
[![Version](https://img.shields.io/badge/version-v0.8.0-green.svg)](ROADMAP.md)

> **一份给 Rust 学习者的 MMORPG 全栈实战教程** — 从 TCP 网关到游戏逻辑再到 Bevy 客户端，用一份代码讲清楚"百万在线游戏"是怎么做出来的。

**当前版本**: v0.8.0 — 四层框架重构（协议层 + 配置数据层 + 事件总线 + 领域拆分）

---

## 目录

- [第 0 章 为什么做这个项目](#第-0-章-为什么做这个项目)
- [第 1 章 前置知识](#第-1-章-前置知识)
- [第 2 章 环境搭建与第一次运行](#第-2-章-环境搭建与第一次运行)
- [第 3 章 架构全景图](#第-3-章-架构全景图)
- [第 4 章 协议设计：让客户端和服务端说同一种语言](#第-4-章-协议设计让客户端和服务端说同一种语言)
- [第 5 章 网关层：TCP 接入与会话管理](#第-5-章-网关层tcp-接入与会话管理)
- [第 6 章 逻辑服：游戏世界的核心](#第-6-章-逻辑服游戏世界的核心)
- [第 7 章 配置数据层：数据驱动游戏](#第-7-章-配置数据层数据驱动游戏)
- [第 8 章 事件总线：解耦业务模块](#第-8-章-事件总线解耦业务模块)
- [第 9 章 Bevy 客户端：用 ECS 做游戏](#第-9-章-bevy-客户端用-ecs-做游戏)
- [第 10 章 测试驱动开发 TDD + BDD](#第-10-章-测试驱动开发-tdd--bdd)
- [第 11 章 集群与生产部署](#第-11-章-集群与生产部署)
- [第 12 章 性能压测与优化](#第-12-章-性能压测与优化)
- [第 13 章 扩展方向与学习路径](#第-13-章-扩展方向与学习路径)
- [附录 A 项目结构总览](#附录-a-项目结构总览)
- [附录 B 常用命令速查](#附录-b-常用命令速查)
- [附录 C 常见问题 FAQ](#附录-c-常见问题-faq)

---

## 第 0 章 为什么做这个项目

### 0.1 这个项目解决了什么问题

市面上大部分 MMORPG 教程要么只讲客户端（Unity/UE 做个 Demo），要么只讲服务端（Netty 写个 Echo），很少有人把"**客户端 → 网关 → 逻辑服 → 数据库**"这条完整链路讲透。

本项目的目标是：

- **全链路贯通**：从 Bevy 客户端的渲染帧，到网关的 TCP 包，到逻辑服的 DashMap，到 PostgreSQL 的行，每一步都能看到代码
- **生产级质量**：不是玩具，80K pps 吞吐、440+ 测试、Docker Compose 部署、Prometheus 监控
- **客户端无关**：服务端用 `proto/game.proto` 作为单一真相源，Unity / Godot / Web 只需重新生成 SDK 即可接入

### 0.2 你能学到什么

| 领域 | 知识点 |
|------|--------|
| Rust 异步 | tokio runtime、async/await、spawn_blocking、task 调度 |
| 并发安全 | DashMap、parking_lot、AtomicU64、crossbeam channel |
| 网络编程 | TCP 粘包/拆包、读写分离、AES-GCM 加密、CRC32 校验 |
| 协议设计 | Protobuf IDL、prost 自动生成、16B 定长包头 |
| 分布式 | gRPC 连接池、Redis PubSub、一致性哈希负载均衡 |
| 游戏架构 | ECS 模式、AOI 九宫格、状态同步、插值平滑 |
| 工程实践 | TDD/BDD、模块化拆分、事件总线、配置驱动 |
| 运维部署 | Docker Compose、Nginx L4、Prometheus、优雅停机 |

### 0.3 项目演进史

| 版本 | 主题 | 教训与收获 |
|------|------|-----------|
| v0.1 | 核心骨架 | 先写协议和加解密，再写业务 |
| v0.2 | 网络+会话 | TCP 读写分离比想象中复杂，心跳保活必须有 |
| v0.3 | 游戏逻辑 | gRPC 路由解耦了网关和逻辑服，真香 |
| v0.4 | 客户端可玩 | JSON 协议到后期维护成本爆炸，必须上 Protobuf |
| v0.5 | 生产化 | Docker/CI/反外挂，上线前最后一公里 |
| v0.6 | 游戏内容 | 内容多了之后单文件 3000 行不可维护，必须拆分 |
| v0.7 | 框架化 | Protobuf 协议层 + 模块化拆分，为客户端切换铺路 |
| v0.8 | 四层架构 | 协议 + 配置 + 事件 + 领域，彻底解耦 |

---

## 第 1 章 前置知识

### 1.1 必须掌握

- **Rust 基础**：所有权、借用、生命周期、`Result<T, E>`、`?` 操作符
- **异步编程**：理解 `async fn`、`Future` trait、`tokio::spawn`
- **TCP 基础**：三次握手、流式传输（无消息边界）、粘包问题
- **JSON / Protobuf**：序列化反序列化的概念

### 1.2 建议了解

- **ECS 模式**：Entity-Component-System，Bevy 的核心思想
- **DashMap**：线程安全的 HashMap，比 `Mutex<HashMap>` 性能好
- **gRPC**：Google 的 RPC 框架，基于 HTTP/2 + Protobuf
- **Redis PubSub**：发布订阅模式，用于跨节点消息广播

### 1.3 推荐学习资源

```bash
# Rust 官方教程
rustup doc --book

# Async Rust
https://tokio.rs/tokio/tutorial

# Bevy 官方手册
https://bevyengine.org/learn/book/

# Protobuf 语言指南
https://protobuf.dev/programming-guides/proto3/
```

---

## 第 2 章 环境搭建与第一次运行

### 2.1 安装 Rust

```bash
# 安装 rustup（Linux/macOS）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Windows 下载 rustup-init.exe
# https://win.rustup.rs/

# 验证
rustc --version   # 应为 1.95.0 或更高
cargo --version
```

本项目使用 `rust-toolchain.toml` 固定版本，进入项目目录会自动切换。

### 2.2 安装 Redis

```bash
# 方式一：Docker（推荐）
docker run -d --name redis-mmo -p 6379:6379 redis:7

# 方式二：本地安装
# Windows: https://github.com/tporadowski/redis/releases
# macOS:   brew install redis && redis-server --daemonize yes
# Linux:   sudo apt install redis-server

# 验证
redis-cli ping   # 返回 PONG
```

### 2.3 克隆并编译

```bash
git clone https://github.com/HavocZhang/mmorpg.git
cd mmorpg

# 编译网关（根 crate）
cargo build --release

# 编译逻辑服
cd logic-lib && cargo build --release --bin logic-server

# 编译 Bevy 客户端
cd ../bevy-client && cargo build --release
```

> **首次编译约需 5-10 分钟**，因为 Bevy 依赖较多。后续增量编译会快很多。

### 2.4 启动三件套

打开 **三个终端**，依次启动：

**终端 1：逻辑服**
```bash
cd logic-lib
cargo run --release --bin logic-server
# 监听 gRPC 127.0.0.1:50051
```

**终端 2：网关**
```bash
cd rust-mmo-gate
cp .env.dev .env
cargo run --release
# 监听 TCP 0.0.0.0:7888
```

**终端 3：Bevy 客户端**
```bash
cd bevy-client
cargo run --release
# 弹出游戏窗口
```

如果一切正常，你会看到：
- 终端 1 输出 `逻辑服启动 gRPC 127.0.0.1:50051`
- 终端 2 输出 `网关监听 TCP 0.0.0.0:7888`
- 游戏窗口显示网格世界、玩家方块（绿色）、NPC（黄色）、怪物（红色）

### 2.5 操作指引

| 按键 | 功能 |
|------|------|
| WASD | 角色移动 |
| 鼠标左键 | 攻击怪物 / 与 NPC 对话 / 拾取掉落 |
| 鼠标滚轮 | 相机缩放 |
| I | 打开/关闭背包 |
| Q | 打开/关闭任务面板 |
| L | 打开/关闭战斗日志 |
| 1-5 | NPC 对话选项 |
| R | 死亡后复活 |

---

## 第 3 章 架构全景图

### 3.1 三层架构

```
┌─────────────────────────────────────────────────────────┐
│                    客户端层 (Bevy)                       │
│  ECS 渲染 + 输入 + UI                                   │
│  TCP 直连 127.0.0.1:7888 + AES-256-GCM 加密             │
└──────────────────────────┬──────────────────────────────┘
                           │ TCP 二进制流
                           ▼
┌─────────────────────────────────────────────────────────┐
│                    网关层 (Gate)                         │
│  TCP 接入 + 握手鉴权 + 会话管理 + 消息路由               │
│  小包合并 + 限流 + 反外挂 + Prometheus 监控              │
│  零业务状态（纯转发）                                    │
└──────────────────────────┬──────────────────────────────┘
                           │ gRPC (tonic)
                           ▼
┌─────────────────────────────────────────────────────────┐
│                  逻辑服层 (Logic Server)                 │
│  GameState (DashMap) + 战斗/背包/任务/NPC/世界          │
│  事件总线 + 配置数据层 + Mob AI                          │
│  PostgreSQL 持久化                                       │
└──────────────────────────┬──────────────────────────────┘
                           │
                           ▼
                    ┌─────────────┐
                    │ PostgreSQL  │  玩家/装备/任务持久化
                    │   Redis     │  集群路由 + PubSub
                    └─────────────┘
```

### 3.2 核心设计原则

**原则 1：网关零业务状态**

网关只做"交警"，不管"车上装的什么"。所有游戏逻辑在逻辑服：

```rust
// src/lib.rs
pub mod game_proto { include!(concat!(env!("OUT_DIR"), "/game.rs")); }
// 网关只引用 proto 类型做转发，不持有任何 GameState
```

**原则 2：协议单一真相源**

一份 `proto/game.proto` 生成所有语言的 SDK：

```
proto/game.proto (35 消息 + 1 枚举 + 包装器)
       │
       ├─→ Rust:    build.rs → prost 自动生成
       ├─→ Bevy:    复用根 crate 的 game_proto 模块
       ├─→ Unity:   protoc --csharp_out（待生成）
       └─→ Godot:   protoc → GDScript（待生成）
```

**原则 3：配置数据驱动**

游戏数值（怪物 HP、技能伤害、NPC 位置）全部在 JSON 配置文件中，不在代码里硬编码：

```
config/*.json  →  config_loader.rs  →  GameState 运行时读取
```

**原则 4：事件总线解耦**

业务模块之间不直接调用，通过事件总线通信：

```rust
// 玩家击杀怪物 → 发布事件
event_bus.publish(GameEvent::MobKilled { entity_id, killer_uid });

// 订阅者各自处理
// - 掉落系统：生成掉落物
// - 广播系统：通知附近玩家
// - 任务系统：更新击杀任务进度
```

### 3.3 三个 Crate 的依赖关系

```
rust-mmo-gate (根 crate: 网关)
    │
    ├── proto/game.proto  →  build.rs 生成 game_proto 模块
    │
    ▲
    │ path = ".."
    │
logic-lib (逻辑服)       bevy-client (客户端)
    │                        │
    │ pub use game_proto      │ pub use game_proto
    │                        │
    └── 复用同一份 proto ─────┘
```

> **关键点**：三个 crate 共享同一份 proto 定义，修改 `proto/game.proto` 后，所有 crate 重新编译即可同步，无需手动拷贝。

---

## 第 4 章 协议设计：让客户端和服务端说同一种语言

### 4.1 为什么不用 JSON

早期版本（v0.1-v0.6）使用 JSON 协议，遇到的问题：

- **体积大**：`{"uid":12345,"name":"玩家","hp":100}` 比 protobuf 多 3-5 倍字节
- **解析慢**：JSON 字符串解析比 protobuf 二进制解析慢 10 倍
- **无类型**：客户端和服务端各自手写 struct，字段对不齐就 bug
- **维护痛**：加一个字段要改 5 个地方（服务端 struct + 客户端 struct + 编码 + 解码 + 文档）

### 4.2 Protobuf：一份 proto 生成所有语言

**第一步：定义消息** `proto/game.proto`

```protobuf
syntax = "proto3";
package mmorpg;

// 玩家属性 (下行 msg_id=5001)
message PlayerStats {
  uint64 uid = 1;
  string name = 2;
  int32  hp = 3;
  int32  max_hp = 4;
  int32  mp = 5;
  int32  max_mp = 6;
  uint32 level = 7;
  uint32 exp = 8;
  uint32 max_exp = 9;
  uint32 gold = 10;
  int32  atk = 11;
  int32  def = 12;
  float  x = 13;
  float  y = 14;
  uint32 skill_cd_1 = 15;
  uint32 skill_cd_2 = 16;
  uint32 skill_cd_3 = 17;
  repeated uint32 talents = 18;  // 天赋列表
}
```

**第二步：自动生成 Rust 代码** `build.rs`

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)
        .compile_protos(&["proto/game.proto"], &["proto/"])?;
    Ok(())
}
```

**第三步：暴露模块** `src/lib.rs`

```rust
pub mod game_proto {
    include!(concat!(env!("OUT_DIR"), "/game.rs"));
}
```

**第四步：使用**

```rust
use rust_mmo_gate::game_proto::PlayerStats;
use prost::Message;

// 编码
let stats = PlayerStats { uid: 12345, name: "玩家".into(), hp: 100, max_hp: 100, ..Default::default() };
let buf: Vec<u8> = stats.encode_to_vec();

// 解码
let decoded = PlayerStats::decode(&buf[..]).unwrap();
assert_eq!(decoded.uid, 12345);
```

### 4.3 16 字节定长包头：解决 TCP 粘包

TCP 是**流式协议**，没有消息边界。发 3 个包可能被合并成 1 个，也可能被拆成 5 个。解决方案：**定长包头 + 变长包体**。

```
┌─────────────── 16 字节包头 ───────────────┬──── 包体 ────┐
│ Magic(2) │ Ver(1) │ Rsv(1) │ MsgId(2) │   BodyLen(2)   │
│          │ CRC32(4)          │ Flags(4) │               │
└──────────────────────────────────────────┴───────────────┘
                                              │
                                              ▼
                          Nonce(12) + Ciphertext + Tag(16)
                          (AES-256-GCM 加密后的 protobuf)
```

| 字段 | 偏移 | 大小 | 说明 |
|------|------|------|------|
| Magic | 0 | 2 | 固定 `0x4D 0x4D` ("MM")，用于识别协议 |
| Version | 2 | 1 | 协议版本号，当前 = 1 |
| Reserved | 3 | 1 | 保留字段 |
| MsgId | 4 | 2 | 消息 ID（大端序），如 5001 = 玩家属性 |
| BodyLen | 6 | 2 | 加密后包体长度 |
| CRC32 | 8 | 4 | 加密后包体的 CRC32 校验值 |
| Flags | 12 | 4 | 标志位，预留 |

**解码器工作流程**：

```rust
// 1. 读够 16 字节包头
let header = read_exact(16).await?;

// 2. 解析 BodyLen，读够包体
let body_len = u16::from_be_bytes([header[6], header[7]]) as usize;
let body = read_exact(body_len).await?;

// 3. CRC32 校验
if crc32fast::hash(&body) != expected_crc {
    return Err("CRC mismatch");
}

// 4. AES-256-GCM 解密
let plaintext = aes_gcm.decrypt(&nonce, &body)?;

// 5. 按 MsgId 用 protobuf 解码
match msg_id {
    5001 => PlayerStats::decode(&plaintext[..]),
    5003 => InventoryUpdate::decode(&plaintext[..]),
    ...
}
```

### 4.4 消息 ID 规约

| 范围 | 方向 | 含义 |
|------|------|------|
| 1-99 | 上行 | 握手/登录/心跳 |
| 100-999 | 上行 | 基础查询（配置拉取等） |
| 1001-1999 | 上行 | 战斗/物品/任务/NPC 操作 |
| 2001-2999 | 上行 | 聊天/组队/公会/职业 |
| 3001-3999 | 上行 | 移动 |
| 4001-4999 | 上行 | 列表查询 |
| 5001-5999 | 下行 | 玩家自身状态 |
| 6001-6999 | 下行 | 战斗结果 |
| 7001-7999 | 下行 | 聊天广播 |
| 8001-8999 | 下行 | 场景动态（玩家/实体位置） |
| 9001-9999 | 下行 | 列表下发 |

### 4.5 统一包装器

所有消息用 `GameMessage` 包装，方便网关层统一转发：

```protobuf
enum MessageDirection {
  UNSPECIFIED = 0;
  UPSTREAM = 1;    // 客户端 → 服务端
  DOWNSTREAM = 2;  // 服务端 → 客户端
}

message GameMessage {
  uint32 msg_id = 1;
  MessageDirection direction = 2;
  bytes payload = 3;        // protobuf 编码后的具体消息
  uint64 target_uid = 4;    // 下行：目标玩家(0=广播)；上行：发送者
}
```

---

## 第 5 章 网关层：TCP 接入与会话管理

### 5.1 网关的职责

网关是"交警"，只管流量不管业务：

1. **接入**：接受 TCP 连接，握手鉴权
2. **会话**：维护 uid ↔ connection 映射，心跳保活
3. **路由**：上行消息转发给逻辑服，下行消息分发给客户端
4. **优化**：小包合并、限流、反外挂
5. **集群**：跨网关消息同步（Redis PubSub）

### 5.2 TCP 接入与握手

```rust
// src/network/tcp_listener.rs
let listener = TcpListener::bind(addr).await?;

loop {
    let (stream, peer_addr) = listener.accept().await?;
    
    // 每个连接 spawn 一个 task
    tokio::spawn(async move {
        // 阶段 1: 握手（验证 Token）
        let session = handshake(stream, peer_addr).await?;
        
        // 阶段 2: 拆分为读写两半
        let (read_half, write_half) = stream.into_split();
        
        // 读循环：接收客户端消息 → 转发给逻辑服
        tokio::spawn(read_loop(read_half, session.clone()));
        
        // 写循环：接收逻辑服消息 → 发给客户端
        tokio::spawn(write_loop(write_half, session.clone()));
    });
}
```

### 5.3 会话管理：DashMap 双映射

需要两种查询方式：`uid → Session` 和 `connection_id → Session`，用两个 DashMap：

```rust
// src/session/session_mgr.rs
pub struct SessionManager {
    /// uid → Session (逻辑服下行时按 uid 查)
    uid_to_session: DashMap<u64, Arc<Session>>,
    /// connection_id → uid (网关内部按连接查)
    conn_to_uid: DashMap<u64, u64>,
}

impl SessionManager {
    /// 玩家登录后绑定
    pub fn bind(&self, uid: u64, conn_id: u64, session: Arc<Session>) {
        self.uid_to_session.insert(uid, session);
        self.conn_to_uid.insert(conn_id, uid);
    }
    
    /// 玩家离线后解绑
    pub fn unbind(&self, uid: u64) {
        self.uid_to_session.remove(&uid);
        // 注意：conn_to_uid 需要先查到 conn_id 再删
    }
}
```

> **为什么用 DashMap 而不是 `Mutex<HashMap>`**？
> 
> `Mutex` 是全局一把锁，所有线程串行访问。`DashMap` 内部分片（默认 16 个 shard），不同 key 可以并行访问，高并发下性能提升 10 倍以上。

### 5.4 小包合并：减少系统调用

玩家移动时每帧发一个移动包（50 字节），如果直接发，每秒 60 个包 = 60 次 `sendto` 系统调用。合并后 16ms 窗口内的包合成一个：

```rust
// src/io_engine/packet_merge.rs
pub struct PacketMerger {
    window_ms: u64,           // 合并窗口，默认 16ms
    buffer: Vec<Packet>,      // 待合并的包
    max_merged_size: usize,   // 单个合并包最大尺寸
}

// 每 16ms flush 一次
loop {
    tokio::time::sleep(Duration::from_millis(16)).await;
    let merged = merger.flush();
    if !merged.is_empty() {
        stream.write_all(&merged).await?;
    }
}
```

**压测数据**：合并前 60 包/秒，合并后 2 包/秒，压缩率 73.37%。

### 5.5 限流与反外挂

```rust
// src/security/rate_limit.rs
pub struct RateLimiter {
    /// 每个玩家的令牌桶
    player_buckets: DashMap<u64, TokenBucket>,
    /// 普通消息 1000/s，战斗消息 2000/s
    default_rate: u32,
    battle_rate: u32,
}

// 每条消息进来都检查
if !limiter.check(uid, msg_id) {
    warn!("玩家 {} 触发限流", uid);
    return Err(RateLimitExceeded);
}
```

反外挂还包含：速度校验（移动距离/时间）、背包校验、频率校验。

---

## 第 6 章 逻辑服：游戏世界的核心

### 6.1 GameState：游戏世界的全部状态

```rust
// logic-lib/src/bin/logic_server/state.rs
pub struct GameState {
    /// 所有在线玩家 (uid → PlayerState)
    pub players: DashMap<u64, PlayerState>,
    /// 所有怪物 (entity_id → MobEntity)
    pub mobs: DashMap<u64, MobEntity>,
    /// 所有 NPC (entity_id → NpcEntity)
    pub npcs: DashMap<u64, NpcEntity>,
    /// 地上掉落物 (drop_id → DropItem)
    pub drops: DashMap<u64, DropItem>,
    
    /// 下一个实体 ID (原子自增)
    pub next_entity_id: AtomicU64,
    /// 下一个掉落物 ID
    pub next_drop_id: AtomicU64,
    
    /// 事件总线
    pub event_bus: EventBus,
    /// 配置数据
    pub config: GameConfig,
}
```

> **为什么用 DashMap 而不是 `RwLock<HashMap>`**？
>
> `RwLock` 读可以并发，但写独占。游戏场景中"怪物 AI tick"每 150ms 遍历所有怪物，如果用 `RwLock` 会阻塞写操作。`DashMap` 的 `iter()` 不持锁，遍历时用 `get_mut()` 逐个加锁，粒度更细。

### 6.2 消息分发：handlers.rs

所有上行消息进入 `process_message`，按 msg_id 路由到对应领域：

```rust
// logic-lib/src/bin/logic_server/handlers.rs
impl GameState {
    pub fn process_message(&self, uid: u64, msg: UpstreamMsg) -> Vec<DownstreamMsg> {
        match msg {
            UpstreamMsg::Login(req)          => self.handle_login(uid, req),
            UpstreamMsg::Attack(req)         => self.handle_attack(uid, req),
            UpstreamMsg::SkillAttack(req)    => self.handle_skill_attack(uid, req),
            UpstreamMsg::Pickup(req)         => self.handle_pickup(uid, req),
            UpstreamMsg::Equip(req)          => self.handle_equip(uid, req),
            UpstreamMsg::AcceptQuest(req)    => self.handle_accept_quest(uid, req),
            UpstreamMsg::CompleteQuest(req)  => self.handle_complete_quest(uid, req),
            UpstreamMsg::NpcInteract(req)    => self.handle_npc_interact(uid, req),
            UpstreamMsg::UseItem(req)        => self.handle_use_item(uid, req),
            UpstreamMsg::ShopBuy(req)        => self.handle_shop_buy(uid, req),
            UpstreamMsg::ShopSell(req)       => self.handle_shop_sell(uid, req),
            UpstreamMsg::Enhance(req)        => self.handle_enhance(uid, req),
            UpstreamMsg::Move(req)           => self.handle_move(uid, req),
            // ...
        }
    }
}
```

### 6.3 战斗系统：combat.rs

```rust
impl GameState {
    pub fn handle_attack(&self, uid: u64, req: AttackRequest) -> Vec<DownstreamMsg> {
        // 1. 查玩家
        let player = self.players.get(&uid)?;
        
        // 2. 查目标怪物
        let mob = self.mobs.get(&req.target_entity_id)?;
        
        // 3. 距离校验
        let dist = distance(player.x, player.y, mob.x, mob.y);
        if dist > player.attack_range + mob.radius {
            return vec![CombatResult { error: "距离太远".into(), .. }.into()];
        }
        
        // 4. 计算伤害（攻击 - 防御，最小 1）
        let damage = (player.atk - mob.def).max(1);
        
        // 5. 暴击判定
        let crit = rand::thread_rng().gen_ratio(1, 5);  // 20% 暴击
        let final_damage = if crit { damage * 2 } else { damage };
        
        // 6. 扣血（短锁！）
        {
            let mut mob = self.mobs.get_mut(&req.target_entity_id)?;
            mob.hp -= final_damage;
            if mob.hp <= 0 {
                // 死亡：发布事件，不在这里处理掉落
                self.event_bus.publish(GameEvent::MobKilled {
                    entity_id: mob.entity_id,
                    killer_uid: uid,
                });
            }
        }
        
        // 7. 构造返回消息（在锁外）
        vec![CombatResult { damage: final_damage, crit, .. }.into()]
    }
}
```

> **锁竞争教训**：早期版本在持锁状态下构造返回消息（包含字符串 clone、Vec 分配），导致并发攻击+任务提交时死锁。修复方案：**锁内只改数据，锁外构造消息**。

### 6.4 怪物 AI：world.rs

怪物 AI 在独立 OS 线程运行（不用 tokio task，避免阻塞 runtime）：

```rust
// logic-lib/src/bin/logic_server/world.rs

/// 怪物 AI tick (每 150ms 执行一次)
pub fn tick_mob_ai(state: Arc<GameState>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(150));
        
        // 1. 先收集所有怪物 ID（不持锁）
        let mob_ids: Vec<u64> = state.mobs.iter().map(|m| m.entity_id).collect();
        
        // 2. 逐个处理（短锁）
        for eid in mob_ids {
            if let Some(mut mob) = state.mobs.get_mut(&eid) {
                match mob.state {
                    0 => { /* idle: 随机巡逻 */ }
                    1 => { /* patrolling: 向目标点移动 */ }
                    2 => { /* chasing: 追击玩家 */ }
                    3 => { /* attacking: 攻击玩家 */ }
                    4 => { /* dead: 等待复活 */ }
                    _ => {}
                }
            }
        }
    });
}
```

> **为什么用 `std::thread` 而不是 `tokio::spawn`**？
>
> 怪物 AI 是 CPU 密集型 + 同步逻辑，用 tokio task 会阻塞 runtime 的其他异步任务（网络 I/O）。独立线程不影响 tokio 调度。

### 6.5 任务系统：quest.rs

```rust
impl GameState {
    pub fn handle_complete_quest(&self, uid: u64, req: CompleteQuestRequest) -> Vec<DownstreamMsg> {
        // 阶段 1: 状态修改（短锁）
        let (rewards, player_info) = {
            let mut player = self.players.get_mut(&uid)?;
            let quest = player.quests.iter().find(|q| q.quest_id == req.quest_id)?;
            
            if !quest.completed {
                return vec![CombatResult { error: "任务未完成".into(), .. }.into()];
            }
            
            // 发放奖励
            player.exp += quest.exp_reward;
            player.gold += quest.gold_reward;
            // ... 装备奖励
            
            (rewards, (player.exp, player.gold, player.level))
        };
        
        // 阶段 2: 消息构造（无锁）
        vec![
            PlayerStats { exp, gold, level, .. }.into(),  // 更新属性
            QuestUpdate { .. }.into(),                     // 更新任务列表
        ]
    }
}
```

### 6.6 事件总线解耦

v0.8 之前，击杀怪物的逻辑耦合在 `handle_attack` 里：扣血 → 死亡判断 → 生成掉落 → 广播 → 任务进度。一个函数 200 行，改一处影响全局。

v0.8 用事件总线解耦：

```rust
// logic-lib/src/bin/logic_server/event_bus.rs
pub enum GameEvent {
    MobKilled { entity_id: u64, killer_uid: u64 },
    ItemPicked { drop_id: u64, picker_uid: u64 },
    QuestProgressed { uid: u64, quest_id: u32, progress: u32 },
}

pub struct EventBus {
    subscribers: Vec<Box<dyn Fn(&GameEvent) + Send + Sync>>,
}

impl EventBus {
    pub fn publish(&self, event: GameEvent) {
        for sub in &self.subscribers {
            sub(&event);
        }
    }
}
```

订阅者各自独立：
- **掉落系统**：收到 `MobKilled` → 查怪物掉落表 → 生成掉落物
- **广播系统**：收到 `MobKilled` → 通知附近玩家
- **任务系统**：收到 `MobKilled` → 更新击杀任务进度

---

## 第 7 章 配置数据层：数据驱动游戏

### 7.1 为什么配置和代码分离

早期版本把怪物属性硬编码在 `constants.rs`：

```rust
// v0.6 的做法（已废弃）
const MOBS: &[MobDef] = &[
    MobDef { id: 1, name: "史莱姆", hp: 50, atk: 8, ... },
    MobDef { id: 2, name: "哥布林", hp: 80, atk: 12, ... },
];
```

问题：
- 改数值要重新编译（5 分钟）
- 策划无法直接修改
- 无法热更新

### 7.2 JSON 配置文件

v0.8 将所有数值抽到 `config/*.json`：

```json
// config/mobs.json
[
  {
    "id": 1,
    "name": "史莱姆",
    "max_hp": 50,
    "atk": 8,
    "def": 2,
    "exp": 20,
    "level": 1,
    "radius": 80.0,
    "detect_range": 120.0,
    "attack_range": 30.0,
    "attack_cd_ms": 2000,
    "move_speed": 0.8
  },
  {
    "id": 8,
    "name": "暗黑巫妖王",
    "max_hp": 2000,
    "atk": 65,
    "def": 30,
    "exp": 1000,
    "level": 20
  }
]
```

### 7.3 9 个配置文件

| 文件 | 内容 | 示例 |
|------|------|------|
| `items.json` | 物品定义 | 名称/类型/属性/图标 |
| `quests.json` | 任务定义 | 目标/奖励/前置任务 |
| `mobs.json` | 怪物定义 | HP/攻击/掉落表 |
| `npcs.json` | NPC 定义 | 位置/对话/功能类型 |
| `maps.json` | 地图定义 | 传送门/区域 |
| `skills.json` | 技能定义 | 伤害/冷却/MP 消耗 |
| `classes.json` | 职业定义 | 初始属性/天赋树 |
| `talents.json` | 天赋定义 | 效果/层级 |
| `shop_items.json` | 商店商品 | 价格/库存 |

### 7.4 配置加载器

```rust
// logic-lib/src/bin/logic_server/config_loader.rs
pub struct GameConfig {
    pub items: Vec<ItemConfig>,
    pub quests: Vec<QuestConfig>,
    pub mobs: Vec<MobConfig>,
    pub npcs: Vec<NpcConfig>,
    pub skills: Vec<SkillConfig>,
    pub classes: Vec<ClassDef>,
    pub shop_items: Vec<ShopItemConfig>,
}

impl GameConfig {
    pub fn load() -> Self {
        Self {
            items: load_json("config/items.json").unwrap_or_default(),
            mobs: load_json("config/mobs.json").unwrap_or_default(),
            // ... 缺失时回退到 constants.rs 的默认值
        }
    }
}

fn load_json<T: DeserializeOwned>(path: &str) -> Option<T> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}
```

### 7.5 客户端拉取配置

客户端启动后通过 `msg_id=101` 拉取全部配置：

```
客户端                          服务端
  │                              │
  │── 101 (PullConfig) ─────────>│
  │                              │ 读取 config/*.json
  │<── 9100 (ConfigBatch) ───────│
  │  items + quests + mobs + ... │
  │                              │
```

---

## 第 8 章 事件总线：解耦业务模块

### 8.1 为什么要事件总线

假设玩家击杀怪物，需要做 4 件事：
1. 扣怪物的血
2. 怪物死亡 → 生成掉落物
3. 广播给附近玩家
4. 更新击杀任务进度

**没有事件总线**（v0.7）：

```rust
fn handle_attack(&self, uid, req) {
    mob.hp -= damage;
    if mob.hp <= 0 {
        self.generate_drops(mob);           // 耦合
        self.broadcast_mob_death(mob);      // 耦合
        self.update_quest_progress(uid, mob); // 耦合
    }
}
```

问题：一个函数调 3 个子系统，改一个影响全部。

**有事件总线**（v0.8）：

```rust
fn handle_attack(&self, uid, req) {
    mob.hp -= damage;
    if mob.hp <= 0 {
        self.event_bus.publish(GameEvent::MobKilled { entity_id, killer_uid: uid });
    }
    // handle_attack 只管战斗，不管后续
}

// 订阅者独立处理
event_bus.subscribe(|event| {
    if let GameEvent::MobKilled { entity_id, killer_uid } = event {
        self.generate_drops(*entity_id);
    }
});
```

### 8.2 事件定义

```rust
pub enum GameEvent {
    /// 怪物被击杀
    MobKilled { entity_id: u64, killer_uid: u64 },
    /// 物品被拾取
    ItemPicked { drop_id: u64, picker_uid: u64 },
    /// 任务进度更新
    QuestProgressed { uid: u64, quest_id: u32, progress: u32 },
}
```

### 8.3 订阅者

3 个独立订阅者，互不依赖：

```rust
// 订阅者 1: 掉落系统
event_bus.subscribe(|event| match event {
    GameEvent::MobKilled { entity_id, .. } => {
        let drops = lookup_drop_table(*entity_id);
        for drop in drops { self.drops.insert(drop.id, drop); }
    }
    _ => {}
});

// 订阅者 2: 广播系统
event_bus.subscribe(|event| match event {
    GameEvent::MobKilled { entity_id, .. } => {
        self.broadcast_to_nearby(*entity_id, EntityDeath { .. });
    }
    _ => {}
});

// 订阅者 3: 任务系统
event_bus.subscribe(|event| match event {
    GameEvent::MobKilled { killer_uid, entity_id } => {
        self.update_kill_quest(*killer_uid, *entity_id);
    }
    _ => {}
});
```

---

## 第 9 章 Bevy 客户端：用 ECS 做游戏

### 9.1 ECS 模式

ECS = Entity + Component + System：

- **Entity**：一个 ID，什么都没有
- **Component**：数据片段，挂在 Entity 上（如 `Health { hp: 100 }`）
- **System**：逻辑，查询特定 Component 组合的 Entity 并处理

```rust
// 每帧自动调用：找到所有有 Health 的实体，HP <= 0 的标记死亡
fn death_system(mut query: Query<(Entity, &Health), Mutated<Health>>) {
    for (entity, health) in &query {
        if health.hp <= 0 {
            commands.entity(entity).insert(Dead);
        }
    }
}
```

### 9.2 项目结构

```
bevy-client/src/
├── main.rs          # 入口：注册插件、系统、资源
├── network.rs       # 网络层：TCP 直连 + AES 加密 + 读写任务分离
├── codec.rs         # 协议层：14 上行编码 + 13 下行解码
├── crypto.rs        # 加密层：AES-256-GCM + CRC32
├── components.rs    # 组件：Player / GameEntity / HealthBar / NameTag
├── resources.rs     # 资源：PlayerState / EntityManager / Inventory
├── systems.rs       # 系统：渲染 / 输入 / 相机 / 插值 / 飘字
└── ui.rs            # UI：HUD / 背包 / 任务 / 对话框
```

### 9.3 网络层架构

Bevy 主线程 60 FPS，不能阻塞在网络 I/O 上。解决方案：**独立线程跑 tokio，双 channel 通信**。

```
┌─────────────────┐     tokio mpsc      ┌──────────────────┐
│   Bevy 主线程    │ ──────────────────> │  网络线程         │
│  (渲染 + 输入)   │   NetworkCommand    │  (tokio runtime) │
│                  │ <────────────────── │                   │
│                  │   crossbeam        │  ReadHalf         │
│                  │   NetworkEvent     │  WriteHalf        │
└─────────────────┘                     └──────────────────┘
```

为什么用两种 channel？
- **tokio mpsc**（命令通道）：网络线程 `await`，不忙等
- **crossbeam**（事件通道）：Bevy 主线程 `try_recv`，不阻塞渲染

### 9.4 实体渲染：父子层级

每个游戏实体是一个父实体 + 多个子实体：

```
Player (父: Sprite + GamePosition + TargetPosition + HealthBar)
├── HP 条背景 (子: Sprite 黑色)
│   └── HP 条前景 (孙: Sprite 绿色，宽度随 HP 变化)
├── 名称标签 (子: Text2dBundle)
└── 选中光环 (子: Sprite 半透明)
```

父实体移动时，子实体自动跟随（Bevy 的 `Transform` 父子绑定）。

### 9.5 位置插值：丝滑移动

服务端每 200ms 发一次位置更新，如果客户端直接跳转，看起来像"瞬移"。解决方案：**目标位置 + 插值**。

```rust
/// 目标位置组件
#[derive(Component)]
pub struct TargetPosition {
    pub x: f32,
    pub y: f32,
}

/// 插值系统：每帧把 Transform 朝 TargetPosition lerp
fn interpolate_position_system(
    mut query: Query<(&mut Transform, &TargetPosition)>,
    time: Res<Time>,
) {
    for (mut transform, target) in &mut query {
        let lerp_factor = 0.15;  // 15% per frame
        transform.translation.x = transform.translation.x.lerp(target.x, lerp_factor);
        transform.translation.y = transform.translation.y.lerp(target.y, lerp_factor);
    }
}
```

### 9.6 相机跟随

```rust
fn camera_follow_system(
    player: Res<PlayerState>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
) {
    let target_x = player.x;
    let target_y = -player.y;  // 游戏坐标 y 向下，Bevy y 向上
    
    for mut transform in &mut camera {
        // 远距离直接吸附，近距离平滑跟随
        let dist = (target_x - transform.translation.x).abs();
        if dist > 300.0 {
            transform.translation.x = target_x;
        } else {
            transform.translation.x = transform.translation.x.lerp(target_x, 0.15);
        }
    }
}
```

---

## 第 10 章 测试驱动开发 TDD + BDD

### 10.1 测试金字塔

```
         ┌───────────┐
         │    E2E    │  7 套件 (需 Redis + 网关运行)
         ├───────────┤
         │    BDD    │  10 个 .feature 场景 (cucumber)
         ├───────────┤
         │ Concurrent│  并发安全测试
         ├───────────┤
         │    TDD    │  13 个单元测试套件
         └───────────┘
```

**总计 440+ 测试用例，0 失败。**

### 10.2 TDD：先写测试再写实现

```rust
// tests/tdd_unit/crypto_tests.rs
#[test]
fn aes_gcm_encrypt_decrypt_roundtrip() {
    let key = [0x42u8; 32];
    let plaintext = b"hello world";
    
    let (ciphertext, nonce) = AesGcm::encrypt(&key, plaintext).unwrap();
    let decrypted = AesGcm::decrypt(&key, &nonce, &ciphertext).unwrap();
    
    assert_eq!(decrypted, plaintext);
}

#[test]
fn aes_gcm_wrong_key_fails() {
    let key1 = [0x42u8; 32];
    let key2 = [0x43u8; 32];
    let plaintext = b"hello";
    
    let (ciphertext, nonce) = AesGcm::encrypt(&key1, plaintext).unwrap();
    let result = AesGcm::decrypt(&key2, &nonce, &ciphertext);
    
    assert!(result.is_err());  // 解密应该失败
}
```

### 10.3 BDD：用自然语言描述行为

```gherkin
# tests/bdd_feature/connect.feature
Feature: TCP连接与握手鉴权

  Scenario: 正常TCP连接建立并进入握手阶段
    Given 客户端发起TCP连接到网关
    When 网关接受连接
    Then 连接应成功建立
    And 连接应进入握手阶段

  Scenario: 非法Token被拒绝
    Given 客户端发起TCP连接到网关
    When 客户端发送握手包 with token "short"
    Then 网关应返回错误 "token长度不足"
    And 连接应被关闭
```

```rust
// tests/bdd/steps/connect_steps.rs
#[given("客户端发起TCP连接到网关")]
async fn client_connects(world: &mut World) {
    world.stream = TcpStream::connect("127.0.0.1:7888").await.ok();
}

#[then("连接应成功建立")]
async fn connection_established(world: &mut World) {
    assert!(world.stream.is_some());
}
```

### 10.4 并发测试：验证无死锁

```rust
// tests/tdd_concurrent/concurrency_tests.rs
#[test]
fn concurrent_attack_and_quest_no_deadlock() {
    let state = Arc::new(GameState::new());
    
    // 8 个线程同时攻击
    let mut handles = vec![];
    for i in 0..8 {
        let s = state.clone();
        handles.push(std::thread::spawn(move || {
            for _ in 0..1000 {
                s.handle_attack(i, AttackRequest { target_entity_id: 1 });
            }
        }));
    }
    
    // 同时提交任务
    let s = state.clone();
    handles.push(std::thread::spawn(move || {
        for _ in 0..1000 {
            s.handle_complete_quest(1, CompleteQuestRequest { quest_id: 1 });
        }
    }));
    
    // 5 秒内必须完成，否则死锁
    for h in handles {
        h.join().expect("线程 panic");
    }
}
```

### 10.5 运行测试

```bash
# 全部单元测试
cargo test --lib

# 特定模块
cargo test --test tdd_crypto

# BDD（需先启动 Redis + 网关）
cargo test --test bdd

# 并发测试
cargo test --test tdd_concurrent

# logic-lib 业务测试
cd logic-lib && cargo test --bin logic-server
```

---

## 第 11 章 集群与生产部署

### 11.1 多节点集群

```
         ┌─────────┐
         │ Nginx   │  L4 TCP 负载均衡
         │ (7888)  │
         └────┬────┘
              │
    ┌─────────┼─────────┐
    │         │         │
┌───▼───┐ ┌──▼───┐ ┌──▼───┐
│Gate#1 │ │Gate#2│ │Gate#3│  3 个网关节点
└───┬───┘ └──┬───┘ └──┬───┘
    └─────────┼─────────┘
              │
       ┌──────▼──────┐
       │ Redis 集群  │  Sentinel 哨兵 + PubSub
       └──────┬──────┘
              │
    ┌─────────┼─────────┐
    │         │         │
┌───▼───┐ ┌──▼───┐ ┌──▼───┐
│Logic#1│ │Logic#2│ │Logic#3│  3 个逻辑服
└───┬───┘ └──┬───┘ └──┬───┘
    └─────────┼─────────┘
              │
       ┌──────▼──────┐
       │ PostgreSQL  │  主从复制
       └─────────────┘
```

### 11.2 跨网关消息：Redis PubSub

玩家 A 在 Gate#1，玩家 B 在 Gate#2，A 给 B 发消息：

```
Gate#1 ──publish──> Redis ──subscribe──> Gate#2 ──> 玩家 B
```

```rust
// src/cluster/cross_gate_pubsub.rs
pub async fn publish_message(redis: &redis::Client, target_uid: u64, msg: &[u8]) {
    let mut conn = redis.get_async_connection().await.unwrap();
    let _: () = conn.publish("game_messages", msg).await.unwrap();
}

pub async fn subscribe_messages(redis: &redis::Client, self_node_id: u32) {
    let mut pubsub = redis.get_async_pubsub().await.unwrap();
    pubsub.subscribe("game_messages").await.unwrap();
    
    loop {
        let msg = pubsub.on_message().next().await;
        let payload = msg.get_payload::<Vec<u8>>();
        
        // 自忽略：不处理自己发的消息
        if from_node != self_node_id {
            route_to_player(payload);
        }
    }
}
```

### 11.3 Docker Compose 一键部署

```bash
# 生产环境部署
cp .env.prod.example .env.prod  # 编辑密钥（必须改 AES_KEY！）
docker compose -f docker-compose.prod.yml up -d
```

服务列表：

| 服务 | 数量 | 端口 |
|------|------|------|
| rust-mmo-gate | 3 | 7888-7890 |
| Redis Sentinel | 1主2从3哨兵 | 6379, 26379 |
| PostgreSQL | 1 | 5432 |
| Nginx | 1 | 7888 (L4) |
| Prometheus | 1 | 9090 |
| Grafana | 1 | 3000 |
| Alertmanager | 1 | 9093 |

### 11.4 生产环境配置要点

```bash
# .env.prod 必须修改的关键项

# 1. AES 密钥（必须用 openssl 生成）
AES_KEY=$(openssl rand -hex 32)

# 2. Redis 集群模式
REDIS_CLUSTER=true
REDIS_URL=redis://redis-master:6379

# 3. 至少 2 个逻辑服容灾
GRPC_LOGIC_ENDPOINTS=grpc://logic-1:50051,grpc://logic-2:50051

# 4. 生产限流（比开发严格 30 倍）
RATE_LIMIT_PLAYER_PER_SEC=30
IP_CONNECT_MAX=20

# 5. 日志格式 JSON（便于 ELK 采集）
LOG_FORMAT=json
LOG_LEVEL=info
```

---

## 第 12 章 性能压测与优化

### 12.1 吞吐压测

| 连接数 | 吞吐量 | 工具 | 时长 |
|--------|--------|------|------|
| 100 | 60,120 pps | Node.js 客户端 | 5s |
| 500 | 53,139 pps | Node.js 客户端 | 10s |
| 200 | **80,862 pps** | Rust bench | 5s (peak) |

> **门禁达成**: ≥ 80,000 pps

### 12.2 稳定性压测

| 指标 | 结果 |
|------|------|
| 并发连接 | 2,500 |
| 连接成功率 | 100% (0 failures) |
| 崩溃次数 | 0 |
| 运行时长 | 56 分钟 |
| 小包合并压缩率 | 73.37% |

### 12.3 关键优化点

**1. DashMap 替代 `Mutex<HashMap>`**
- 分片锁，不同 key 并行访问
- 高并发下 10 倍性能提升

**2. 小包合并**
- 16ms 窗口内的小包合成一个大包
- 减少 `sendto` 系统调用 73%

**3. 锁粒度最小化**
- 锁内只做数据修改，锁外构造消息
- 避免在持锁状态下做字符串 clone / Vec 分配

**4. 怪物 AI 独立线程**
- 不占用 tokio runtime
- 150ms tick + AtomicU64 时间戳节流

**5. gRPC 连接池**
- 预建连接，避免每次 RPC 都握手
- 一致性哈希负载均衡

### 12.4 操作延迟基准

| 操作 | 延迟 |
|------|------|
| AES-256-GCM 加密 (1KB) | 12 µs |
| AES-256-GCM 解密 (1KB) | 57 µs |
| CRC32 (4KB) | 186 µs |
| 协议编码 (512B) | 355 µs |
| 协议解码 (512B) | 520 µs |
| 小包合并 (10 packets) | 409 µs |
| 雪花 ID 生成 | 508 µs (batch) |
| IP 黑名单查询 (10K entries) | 1.9 ms |

---

## 第 13 章 扩展方向与学习路径

### 13.1 接下来可以做什么

**玩法扩展**：
- [ ] Buff/Debuff 系统（中毒/眩晕/加速）
- [ ] 装备随机词缀 + 套装效果
- [ ] 副本系统（独立场景实例）
- [ ] 世界 Boss（全服广播 + 伤害排行）
- [ ] PvP 竞技场（匹配 + 排位）
- [ ] 公会战（领地争夺）

**技术深化**：
- [ ] AOI 九宫格优化（当前是全量广播）
- [ ] 状态同步改为增量同步（Delta Encoding）
- [ ] 客户端预测 + 服务端校正
- [ ] 数据库读写分离
- [ ] 逻辑服水平扩展（按地图分片）

**客户端扩展**：
- [ ] Unity 客户端（从 game.proto 生成 C# SDK）
- [ ] Godot 客户端（从 game.proto 生成 GDScript）
- [ ] 角色动画（骨骼动画 + 状态机）
- [ ] 音效与 BGM
- [ ] 小地图与迷雾

### 13.2 学习路径建议

```
第 1 周：跑通项目 + 读懂协议
├── 第 1-2 天：环境搭建，三件套跑起来
├── 第 3-4 天：读 proto/game.proto + PROTOCOL.md
└── 第 5-7 天：读懂网关 TCP 接入 + 握手

第 2 周：读懂网关层
├── 网络层：tcp_listener + handshake
├── 会话层：session_mgr (DashMap 双映射)
├── I/O 层：read_loop + write_loop + packet_merge
└── 运行 tdd_network + tdd_session 测试

第 3 周：读懂逻辑服
├── 状态层：GameState (DashMap + AtomicU64)
├── 业务层：handlers → combat/inventory/quest/world
├── 配置层：config_loader.rs + 9 个 JSON
└── 运行 logic-server 测试

第 4 周：读懂客户端
├── 网络层：network.rs (双 channel 架构)
├── ECS 层：components + resources + systems
├── 渲染层：spawn_entity + interpolate_position
└── 尝试修改一个功能（如加一个新技能）
```

### 13.3 推荐进阶阅读

- 《游戏编程模式》— Robert Nystrom
- 《大规模网络游戏开发》— 罗云登
- 《Game Server Development》— Golang 实现但架构思想通用
- Bevy 官方示例：https://github.com/bevyengine/bevy/tree/latest/examples

---

## 附录 A 项目结构总览

```
rust-mmo-gate/
├── src/                          # 网关核心 (13 模块)
│   ├── main.rs                   # 启动入口
│   ├── config/                   # 配置加载
│   ├── foundation/               # 雪花 ID + 日志
│   ├── crypto/                   # AES-256-GCM + CRC32
│   ├── protocol/                 # 16B 包头 + 编解码
│   ├── network/                  # TCP 接入 + 握手
│   ├── session/                  # 会话管理 (DashMap)
│   ├── io_engine/                # 小包合并 + 优先级队列
│   ├── grpc_router/              # gRPC 连接池 + 负载均衡
│   ├── cluster/                  # Redis PubSub + 路由索引
│   ├── security/                 # 限流 + 黑名单 + 审计
│   └── admin/                    # HTTP 监控 + Prometheus
│
├── logic-lib/                    # 游戏逻辑独立 crate
│   └── src/
│       ├── db.rs                 # PostgreSQL + SQLite
│       ├── party.rs              # 组队系统
│       ├── chat/                 # 聊天模块
│       ├── combat/               # 战斗模块
│       ├── scene/                # 场景/AOI
│       └── bin/logic_server/     # 主逻辑服 (v0.8 四层架构)
│           ├── main.rs           # gRPC 入口
│           ├── handlers.rs       # 消息分发
│           ├── combat.rs         # 战斗领域
│           ├── inventory.rs      # 背包/装备/商店
│           ├── quest.rs          # 任务领域
│           ├── world.rs          # NPC/怪物 AI
│           ├── codec.rs          # proto 编解码
│           ├── config_loader.rs  # 配置数据层
│           ├── event_bus.rs      # 事件总线
│           ├── state.rs          # GameState
│           ├── types.rs          # 实体结构
│           ├── constants.rs      # 常量
│           ├── utils.rs          # 工具函数
│           └── tests.rs          # 56 个测试
│
├── bevy-client/                  # Bevy 原生客户端
│   ├── src/
│   │   ├── main.rs               # 入口 + 相机
│   │   ├── network.rs            # TCP + AES + 双 channel
│   │   ├── codec.rs              # 14 上行 + 13 下行
│   │   ├── crypto.rs             # AES-GCM + CRC32
│   │   ├── components.rs         # ECS 组件
│   │   ├── resources.rs          # ECS 资源
│   │   ├── systems.rs            # 20+ 系统
│   │   └── ui.rs                 # HUD + 面板
│   └── assets/fonts/simhei.ttf   # 中文字体
│
├── config/                       # 配置数据层 (9 JSON)
│   ├── items.json
│   ├── quests.json
│   ├── mobs.json
│   ├── npcs.json
│   ├── maps.json
│   ├── skills.json
│   ├── classes.json
│   ├── talents.json
│   └── shop_items.json
│
├── proto/                        # Protobuf 协议定义
│   ├── gate.proto                # 网关 gRPC
│   └── game.proto                # 游戏消息 (35 消息)
│
├── tests/                        # 测试套件
│   ├── tdd_unit/                 # 13 个单元测试
│   ├── tdd_concurrent/           # 并发安全
│   ├── tdd_fuzz/                 # 模糊测试
│   ├── tdd_exception/            # 异常场景
│   ├── bdd/                      # BDD 步骤
│   └── bdd_feature/              # 10 个 .feature
│
├── deploy/                       # 生产部署
│   ├── nginx/
│   ├── prometheus/
│   └── alertmanager/
│
├── docker-compose.prod.yml       # 生产 Docker Compose
├── .github/workflows/ci.yml      # CI/CD
├── ROADMAP.md                    # 路线图
├── PROTOCOL.md                   # 协议文档
└── Cargo.toml
```

---

## 附录 B 常用命令速查

```bash
# ─── 编译 ───
cargo build --release                      # 编译网关
cd logic-lib && cargo build --release --bin logic-server  # 编译逻辑服
cd bevy-client && cargo build --release    # 编译客户端

# ─── 运行 ───
cargo run --release                        # 启动网关 (TCP 7888)
cd logic-lib && cargo run --release --bin logic-server  # 启动逻辑服
cd bevy-client && cargo run --release      # 启动客户端

# ─── 测试 ───
cargo test --lib                           # 全部单元测试
cargo test --test tdd_crypto               # 特定模块
cargo test --test tdd_concurrent           # 并发测试
cargo test --test bdd                      # BDD (需 Redis+网关)
cd logic-lib && cargo test --bin logic-server  # 逻辑服测试

# ─── 检查 ───
cargo check                                # 快速编译检查
cargo clippy --all-targets -- -D warnings  # 零警告
cargo audit                                # 安全扫描
cargo fmt                                  # 格式化

# ─── 集群 ───
GATE_TCP_PORT=7889 GATE_NODE_ID=2 cargo run --release  # 第二个节点

# ─── 部署 ───
docker compose -f docker-compose.prod.yml up -d  # 生产部署
bash ci.sh                                        # CI 检查
```

---

## 附录 C 常见问题 FAQ

### Q1: 编译报错 `protoc not found`

**A**: 安装 protoc 编译器：
```bash
# Windows
choco install protoc

# macOS
brew install protobuf

# Linux
sudo apt install protobuf-compiler
```

### Q2: Bevy 客户端启动黑屏

**A**: 检查 `bevy-client/assets/fonts/simhei.ttf` 是否存在。release 构建需要：
```bash
Copy-Item -Path "assets" -Destination "target\release\assets" -Recurse -Force
```

### Q3: 连接网关失败 "connection refused"

**A**: 按顺序检查：
1. Redis 是否启动：`redis-cli ping`
2. 逻辑服是否启动：`netstat -an | findstr 50051`
3. 网关是否启动：`netstat -an | findstr 7888`

### Q4: 实体不显示（只有网格）

**A**: 已在 v0.8 修复，确保：
- 相机 `near = -1000.0`
- 实体有 `NoFrustumCulling` 组件
- 服务端 9002 消息用 protobuf 编码（不是 JSON）

### Q5: 运行 BDD 测试失败

**A**: BDD 测试需要完整环境：
```bash
# 1. 启动 Redis
docker run -d -p 6379:6379 redis:7

# 2. 启动网关
cargo run --release &

# 3. 再运行测试
cargo test --test bdd
```

### Q6: 如何添加新怪物

**A**: 三步：
1. 编辑 `config/mobs.json`，添加新条目
2. 重启逻辑服（配置在启动时加载）
3. 客户端会通过 9002 消息自动收到新怪物

### Q7: 如何添加新客户端（如 Unity）

**A**: 从 proto 生成 SDK：
```bash
# Unity (C#)
protoc --csharp_out=./Assets/Scripts/Proto proto/game.proto

# Godot (GDScript)
# 使用 grpc-gdscript 或 protobuf-gdscript 插件
```

然后实现 TCP 连接 + 16B 包头 + AES-GCM 加密即可。

---

## 版本历史

| 版本 | 主题 | 核心成果 |
|------|------|----------|
| v0.1 | 核心骨架 | config/crypto/protocol/foundation + 17 TDD |
| v0.2 | 网络+会话 | TCP握手/会话管理/心跳/I/O引擎 |
| v0.3 | 游戏逻辑 | gRPC路由/战斗/场景/聊天/装备/任务 |
| v0.4 | 客户端可玩 | 网页客户端/背包/装备/技能/NPC |
| v0.5 | 生产化 | Docker/CI/反外挂/SQLite |
| v0.6 | 游戏内容 | 4地图/3Boss/公会/PvP/商店/职业 |
| v0.7 | 框架化 | Protobuf 协议层/模块化/稳定性修复 |
| **v0.8** | **四层架构** | **协议+配置+事件+领域+Bevy客户端+TCP统一** |

详细路线图见 [ROADMAP.md](ROADMAP.md)

---

## 许可证

MIT License — 可自由用于学习和商业项目。

---

*最后更新: 2026-07-13 | v0.8.0 | 440+ 测试 | 80K pps | Bevy 原生客户端*
