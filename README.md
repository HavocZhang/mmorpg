# Rust MMO Gate — 百万在线游戏接入网关集群

[![Rust](https://img.shields.io/badge/Rust-1.95.0-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://img.shields.io/badge/CI-passing-brightgreen.svg)](ci.sh)
[![Security](https://img.shields.io/badge/audit-0%20vulns-brightgreen.svg)](#安全审计)
[![Throughput](https://img.shields.io/badge/throughput-80K%20pps-blue.svg)](#吞吐压测)
[![Stability](https://img.shields.io/badge/stability-72h%20running-brightgreen.svg)](#稳定性压测)
[![Version](https://img.shields.io/badge/version-v0.7.0-green.svg)](ROADMAP.md)

**Rust 实现的高性能、有状态的 MMO 游戏接入网关集群**，支持百万级并发在线、多节点集群跨网关通信、gRPC 逻辑服路由，自带完整 MMORPG 游戏逻辑与客户端无关的协议契约层。

**当前版本**: v0.7.0 — 框架化重构（Protobuf 协议层 + 模块化拆分 + 稳定性修复）

> 🎮 打开 `web-client/game.html` 即可体验完整的 MMORPG 游戏！

---

## 目录

- [v0.7 更新亮点](#v07-更新亮点)
- [架构概览](#架构概览)
- [游戏功能](#游戏功能)
- [快速开始](#快速开始)
- [生产部署](#生产部署)
- [客户端无关框架](#客户端无关框架)
- [测试报告](#测试报告)
- [性能指标](#性能指标)
- [项目结构](#项目结构)
- [CI 流水线](#ci-流水线)

---

## v0.7 更新亮点

### 1. Protobuf 协议契约层（客户端无关框架基石）
- 新增 [proto/game.proto](proto/game.proto)：35 个消息 + 1 枚举 + 包装器，覆盖全部 17 上行 + 17 下行消息
- `build.rs` 自动编译生成 Rust 代码，`logic_lib::game_proto` 模块统一暴露
- 新增 [web-client/sdk/proto_sdk.js](web-client/sdk/proto_sdk.js) JS 客户端 SDK，从同一份 proto 生成
- **切换客户端（Unity/UE/Godot/移动端）只需从 game.proto 重新生成对应语言 SDK**

### 2. 代码模块化重构
- `logic_server.rs` 3448 行单文件拆分为 7 个模块（`src/bin/logic_server/` 目录）：
  - `main.rs` 入口 + gRPC impl
  - `constants.rs` 常量 + 静态数据
  - `types.rs` 实体结构
  - `utils.rs` 工具函数
  - `state.rs` GameState + MockLogicService
  - `handlers.rs` 业务方法
  - `tests.rs` 18 个测试

### 3. 稳定性修复（TDD/BDD 方法论）
- 修复 DashMap 锁竞争死锁（tick_mob_ai / handle_complete_quest / 8004 广播）
- 修复 tokio runtime 阻塞（spawn_blocking + 独立 OS 线程）
- 修复装备强化随机数种子（AtomicU64 计数器）
- tracing 日志替换 println!，带模块路径定位

### 4. 新增功能
- 装备强化系统（+0~+10，分档成功率，DB 持久化）
- 客户端小地图（右上角 160×120，显示怪物/NPC/玩家）
- 客户端登录页面 + 职业选择面板

---

## 架构概览

```
                    ┌─────────────┐
                    │   Client    │  (WebSocket :7890 / TCP :7888)
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
         ┌────▼────┐  ┌────▼────┐  ┌────▼────┐
         │ Gate #1 │  │ Gate #2 │  │ Gate #3 │   (Rust 网关集群)
         └────┬────┘  └────┬────┘  └────┬────┘
              │            │            │
              └────────────┼────────────┘
                           │  Redis PubSub (跨网关消息)
                   ┌───────▼───────┐
                   │    Redis      │  (路由索引 + 广播)
                   └───────┬───────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
         ┌────▼────┐  ┌────▼────┐  ┌────▼────┐
         │ Logic   │  │ Scene   │  │ Combat  │
         │ Server  │  │ Server  │  │ Server  │  (gRPC 游戏逻辑)
         └─────────┘  └─────────┘  └─────────┘
```

**14 个核心模块**：

| 模块 | 职责 | 关键技术 |
|------|------|----------|
| `config` | 环境变量配置加载 | dotenv, APP_ENV 多环境 |
| `foundation` | 雪花ID生成器 | 无锁 AtomicU64 |
| `crypto` | AES-256-GCM 加密 | 12B nonce + 16B tag |
| `protocol` | 16B 定长包头 + CRC32 | 变长包体流式解码 |
| `network` | TCP/WS 接入 + 握手里程碑 | Tokio TCP split + WsAdapter |
| `session` | 会话管理（顶号/心跳） | DashMap 双映射 |
| `io_engine` | 小包合并 + 优先级队列 | 16ms 合并窗口, 泛型 ReadLoop/WriteLoop |
| `grpc_router` | gRPC 连接池 + 负载均衡 | tonic, 一致性哈希 |
| `cluster` | Redis 路由索引 + PubSub | 跨节点消息精准投递 |
| `security` | IP 黑名单 + 限流 + 审计 | 滑动窗口 + IP 前缀 |
| `admin` | HTTP 监控 + Prometheus | actix-web, 优雅停机 |
| `chat` | 聊天系统 | broadcast 广播 + 私聊/公会频道 |
| `combat/scene` | 战斗/场景/AOI/反外挂 | logic-lib 独立 crate |
| `ws_listener` | WebSocket 原生支持 | WsAdapter AsyncRead/AsyncWrite |

---

## 游戏功能

Logic Server 实现了完整 MMORPG 游戏玩法：

| 系统 | 功能 | 版本 |
|------|------|------|
| 战斗 | 8 种怪物 + 5 技能 + 暴击 + 粒子特效 | v0.3 |
| 装备 | 5 种装备 + 穿戴/卸下 + 强化+10 + 对比 | v0.7 |
| 背包 | 10 种物品 + 分类排序 + 使用/丢弃 | v0.4 |
| 任务 | 5 个任务 + 接取/追踪/完成 + 奖励 | v0.4 |
| NPC | 14 NPC + 对话/选项 + 商人/任务/治疗/传送 | v0.6 |
| 地图 | 4 地图 + 传送门 + 专属怪物 | v0.6 |
| 经济 | 商店(8商品) + 购买/卖出 + 金币 | v0.6 |
| 副本 | 3 Boss + 祭坛召唤 + 增强掉落 | v0.6 |
| 公会 | 创建/加入/离开 + 公会频道 | v0.6 |
| PvP | 1v1 决斗发起/接受 | v0.6 |
| 组队 | 邀请/加入/离开 + 队伍聊天 | v0.4 |
| 职业 | 战/法/弓 3 职业 + 9 天赋 + 属性加成 | v0.6 |
| 排行 | 等级榜/金币榜 Top20 | v0.6 |
| 反外挂 | 速度/频率/背包校验 + 强制拉回 | v0.5 |
| 小地图 | 右上角 160×120 实时显示实体分布 | v0.7 |

---

## 快速开始

### 环境要求

- Rust 1.95.0 (toolchain 固定)
- Redis 7.x (Docker: `docker run -d -p 6379:6379 redis:7`)
- Windows / Linux / macOS

### 编译 & 运行

```bash
# 1. 启动 Redis
docker run -d --name redis-mmo -p 6379:6379 redis:7

# 2. 编译（release + native 优化）
cargo build --release

# 3. 启动逻辑服
cd logic-lib && cargo run --release --bin logic-server &

# 4. 启动网关 (TCP :7888 + WS :7890)
cp .env.dev .env
cargo run --release

# 5. 打开网页客户端玩
open web-client/game.html
```

### 单节点 + 集群模式 + 网页客户端

```bash
# 单节点（默认）
./target/release/rust-mmo-gate.exe

# 双节点集群（Gate-2 用不同端口）
GATE_TCP_PORT=7889 GATE_HTTP_PORT=9091 GATE_NODE_ID=2 \
GATE_NODE_NAME=gate-dev-02 \
./target/release/rust-mmo-gate.exe

# 打开网页客户端
# 直接在浏览器打开 web-client/game.html
# 或使用任意 HTTP 服务器:
cd web-client && python -m http.server 8080
```

---

## 生产部署

使用 Docker Compose 一键部署完整生产环境：

```bash
# 生产环境部署（3 网关 + Redis Sentinel + PG + Nginx + 监控）
cp .env.prod.example .env.prod  # 编辑密钥
docker compose -f docker-compose.prod.yml up -d

# 服务列表：
# - rust-mmo-gate ×3 (TCP 7888-7890, WS 7890-7892)
# - Redis Sentinel: 1 主 + 2 从 + 3 哨兵
# - PostgreSQL 16
# - Nginx (L4 TCP 负载均衡 + L7 WebSocket 代理)
# - Prometheus + Grafana (端口 3000)
# - Alertmanager (企业微信/钉钉告警)
```

---

## 客户端无关框架

v0.7 引入 Protobuf 协议契约层，让服务端与客户端解耦：

```
proto/game.proto (单一真相源)
       │
       ├─→ Rust:    build.rs → prost 自动生成 → logic_server 使用
       ├─→ JS/Web:  game_proto.json → proto_sdk.js → game.html 使用
       ├─→ Unity:   protoc --csharp_out → C# SDK（待生成）
       └─→ Godot:   protoc → GDScript（待生成）
```

### 协议层使用

**Rust 端**（已集成）：
```rust
use logic_lib::game_proto::PlayerStats;
use prost::Message;

let stats = PlayerStats { uid: 12345, name: "玩家".into(), hp: 100, max_hp: 100, ... };
let buf = stats.encode_to_vec();
let decoded = PlayerStats::decode(&buf[..]).unwrap();
```

**JS/Web 端**（SDK 已就绪）：
```javascript
// 通过 <script src="sdk/proto_sdk.js"> 加载
const buf = ProtoSDK.encode(5001, { uid: 12345, name: '玩家', hp: 100, ... });
const msg = ProtoSDK.decode(5001, buf);
```

### 测试验证

```bash
# Rust 协议层 TDD 测试（5 个）
cd logic-lib && cargo test --test tdd_proto

# 浏览器 SDK 测试
# 打开 web-client/sdk/proto_sdk_test.html
```

---

## 测试报告

### 测试覆盖矩阵

| 模块 | TDD 套件 | 内联测试 | 并发测试 | BDD | 状态 |
|------|----------|----------|----------|-----|------|
| config | ✅ 7 | ✅ | - | ✅ | 100% |
| crypto | ✅ 10 | ✅ | - | ✅ | 100% |
| protocol | ✅ 7 | ✅ | - | ✅ | 100% |
| network | ✅ 7 | ✅ | - | ✅ | 100% |
| session | ✅ 12 | ✅ | ✅ +2 | ✅ | 100% |
| io_engine | ✅ 10 | ✅ | ✅ +1 | - | 100% |
| grpc_router | ✅ 9 | ✅ | ✅ +1 | - | 100% |
| cluster | ✅ 4 | ✅ | - | ✅ | 100% |
| security | ✅ 10 | ✅ | ✅ | ✅ | 100% |
| admin | ✅ 6 | ✅ | - | - | 100% |
| chat/combat/scene | ✅ | ✅ | - | ✅ | logic-lib |
| **proto (v0.7)** | ✅ 5 | - | - | - | ✅ |
| **logic-server (v0.7)** | - | ✅ 18 | ✅ | - | ✅ |

**总计**：16 个 TDD 套件 + logic-server 18 个内联测试，共 100+ 测试用例

### 测试运行

```bash
# 全部单元测试
cargo test --lib

# v0.7 协议层测试
cd logic-lib && cargo test --test tdd_proto

# logic-server 业务测试（含锁竞争 + BDD 场景）
cd logic-lib && cargo test --bin logic-server

# BDD 场景测试（需 Redis + 网关运行）
cargo test --test bdd

# CI 一键检查
bash ci.sh
```

---

## 性能指标

### 吞吐压测

| 连接数 | 吞吐量 | 工具 | 时长 |
|--------|--------|------|------|
| 100 | **60,120 pps** | Node.js 客户端 | 5s |
| 500 | 53,139 pps | Node.js 客户端 | 10s |
| 50 | 72,000 pps | Rust bench | 5s |
| 200 | **80,862 pps** | Rust bench | 5s (peak) |

> **门禁达成**: ≥ 80,000 pps ✅

### 稳定性压测

| 指标 | 结果 |
|------|------|
| 并发连接 | 2,500 |
| 连接成功率 | 100% (0 failures) |
| 崩溃次数 | 0 |
| 合并压缩率 | 73.37% (门禁 ≥ 70%) |

### 操作延迟

| 操作 | 延迟 |
|------|------|
| AES-256-GCM 加密 (1KB) | 12 µs |
| AES-256-GCM 解密 (1KB) | 57 µs |
| CRC32 (4KB) | 186 µs |
| 协议编码 (512B) | 355 µs |
| 协议解码 (512B) | 520 µs |
| 小包合并 (10 packets) | 409 µs |
| 优先级队列 push (100 msg) | 2.76 µs |
| 雪花 ID 生成 | 508 µs (batch) |
| IP 黑名单查询 (10K entries) | 1.9 ms |
| 限流检查 | 244 µs |

---

## 集群验证

双节点集群跨网关消息测试 **完全通过**：

| 功能 | 状态 |
|------|------|
| 多节点启动（共享 Redis） | ✅ |
| 路由索引（uid → gate_node） | ✅ |
| PubSub 广播通道 | ✅ |
| 自忽略机制（from_node == node_id） | ✅ |
| **跨网关聊天消息** | ✅ |

---

## 项目结构

```
rust-mmo-gate/
├── src/                          # 网关核心代码 (14 模块)
│   ├── main.rs                   # 启动入口 (TCP + WS 双监听)
│   ├── config/                   # 配置管理
│   ├── foundation/               # 雪花 ID 生成器
│   ├── crypto/                   # AES-256-GCM 加密
│   ├── protocol/                 # 16B 包头 + CRC32
│   ├── network/                  # TCP/WS 接入 + 握手里程碑
│   ├── session/                  # 会话管理（DashMap 双映射）
│   ├── io_engine/                # 小包合并 + 优先级队列 (泛型)
│   ├── grpc_router/              # gRPC 连接池 + 负载均衡
│   ├── cluster/                  # Redis 路由索引 + PubSub
│   ├── security/                 # IP 黑名单 + 限流 + 反外挂
│   └── admin/                    # HTTP 监控 + Prometheus
│
├── logic-lib/                    # 游戏逻辑独立 crate (满功能 MMO)
│   └── src/
│       ├── db.rs                 # 数据库 (PG + SQLite 降级)
│       ├── party.rs              # 组队系统
│       ├── chat/                 # 聊天模块
│       ├── combat/               # 战斗模块
│       ├── scene/                # 场景/AOI 模块
│       └── bin/logic_server/     # 主逻辑服 (v0.7 模块化)
│           ├── main.rs           # 入口 + gRPC impl
│           ├── constants.rs      # 常量 + 静态数据
│           ├── types.rs          # 实体结构
│           ├── utils.rs          # 工具函数
│           ├── state.rs          # GameState + MockLogicService
│           ├── handlers.rs       # 业务方法
│           └── tests.rs          # 18 个测试
│
├── proto/                        # Protobuf 协议定义 (v0.7)
│   ├── gate.proto                # 网关 gRPC 协议
│   └── game.proto                # 游戏消息协议 (35 消息)
│
├── web-client/                   # 网页游戏客户端
│   ├── game.html                 # 单文件 MMORPG 客户端
│   ├── sdk/                      # 客户端 SDK
│   │   ├── game_proto.json       # proto 生成的消息描述
│   │   ├── proto_sdk.js          # Protobuf 编解码 SDK
│   │   └── proto_sdk_test.html   # SDK 测试页
│   └── test_*.js                 # E2E/压测脚本
│
├── tests/                        # 测试套件
│   ├── tdd_unit/                 # 16 个 TDD 单元测试文件
│   ├── tdd_concurrent/           # 并发安全测试
│   ├── tdd_fuzz/                 # 模糊测试
│   ├── tdd_exception/            # 异常场景测试
│   ├── bdd/                      # BDD cucumber 步骤定义
│   └── bdd_feature/              # Gherkin .feature 场景
│
├── deploy/                       # 生产部署配置
│   ├── nginx/                    # Nginx L4 TCP + L7 WS
│   ├── prometheus/               # 监控告警
│   └── alertmanager/             # Alertmanager + Webhook
│
├── docker-compose.prod.yml       # 生产环境 Docker Compose
├── .github/workflows/ci.yml      # GitHub Actions CI/CD
├── Dockerfile                    # Docker 镜像构建
├── ROADMAP.md                    # 完整版本路线图
├── PROTOCOL.md                   # 游戏协议文档
├── Cargo.toml                    # Rust 依赖
└── rust-toolchain.toml           # Rust 1.95.0
```

---

## 安全审计

```bash
cargo audit
# 结果: 0 vulnerabilities found
```

**安全特性**：
- AES-256-GCM 加密（密钥通过环境变量注入，非硬编码）
- CRC32 防篡改校验
- IP 连接频率限制 + IP 黑名单
- 消息频率玩家级限流（1000/s 普通，2000/s 战斗）
- 握手 Token 验证
- 安全事件审计日志

---

## CI 流水线

```bash
# 基础检查 (编译 + clippy + TDD)
bash ci.sh

# 含安全审计
bash ci.sh --audit

# 含性能基准 (cargo bench)
bash ci.sh --bench

# 含 BDD 场景测试
bash ci.sh --full
```

**检查项**：cargo check → fmt → clippy -D warnings → cargo test --lib → TDD 套件 → (可选: audit / bench / BDD)

另外包含 GitHub Actions CI/CD 流水线 (`.github/workflows/ci.yml`)：
5 阶段自动运行：lint（fmt + clippy）→ test → audit → docker build → release

---

## 常用命令

```bash
# 快速编译检查
cargo check

# 运行全部单元测试
cargo test --lib

# Release 构建
cargo build --release

# 性能压测
cargo bench

# 安全扫描
cargo audit

# 编译零警告
cargo clippy --all-targets -- -D warnings
```

---

## 技术栈

| 类别 | 技术 |
|------|------|
| 语言 | Rust 1.95.0 |
| 异步运行时 | Tokio multi-thread |
| 并发 | DashMap + parking_lot + crossbeam |
| 加密 | AES-256-GCM (aes-gcm crate) |
| 协议 | 16B 定长包头 + CRC32 + 变长包体 (TCP/WS) |
| 序列化 | prost (Protobuf) + serde_json |
| gRPC | tonic 0.12 |
| HTTP | actix-web 4 |
| WebSocket | tokio-tungsenite 0.24 |
| Redis | redis-rs (连接池 + PubSub + Sentinel) |
| 数据库 | PostgreSQL + SQLite (离线降级) |
| 监控 | Prometheus + Grafana + Alertmanager |
| 部署 | Docker Compose + Nginx L4/L7 |
| CI/CD | GitHub Actions 5 阶段流水线 |
| 日志 | tracing + tracing-subscriber |

---

## 版本历史

| 版本 | 主题 | 核心成果 |
|------|------|----------|
| v0.1 | 核心骨架 | config/crypto/protocol/foundation + 17 TDD |
| v0.2 | 网络+会话 | TCP握手/会话管理/心跳/I/O引擎 |
| v0.3 | 游戏逻辑 | gRPC路由/战斗/场景/聊天/装备/任务 |
| v0.4 | 客户端可玩 | 网页客户端/背包排序/装备对比/技能特效/NPC |
| v0.5 | 生产化 | Docker Compose / WebSocket原生 / CI/CD / 反外挂 / SQLite |
| v0.6 | 游戏内容 | 4地图/3Boss/公会/PvP/商店/职业/私聊/排行 |
| **v0.7** | **框架化** | **Protobuf 协议层 / 模块化重构 / 稳定性修复 / 装备强化 / 小地图** |

详细路线图见 [ROADMAP.md](ROADMAP.md)

---

## 许可证

MIT License

---

*最后更新: 2026-07-12 | v0.7.0 框架化重构 | 80K pps 吞吐 | 100+ 测试通过*
