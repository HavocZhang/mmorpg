# Rust MMO Gate — 百万在线游戏接入网关集群

[![Rust](https://img.shields.io/badge/Rust-1.95.0-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://img.shields.io/badge/CI-passing-brightgreen.svg)](ci.sh)
[![Security](https://img.shields.io/badge/audit-0%20vulns-brightgreen.svg)](#安全审计)
[![Throughput](https://img.shields.io/badge/throughput-80K%20pps-blue.svg)](#吞吐压测)
[![Stability](https://img.shields.io/badge/stability-72h%20running-brightgreen.svg)](#稳定性压测)
[![Version](https://img.shields.io/badge/version-v0.8.0-green.svg)](ROADMAP.md)

**Rust 实现的高性能、有状态的 MMO 游戏接入网关集群**，支持百万级并发在线、多节点集群跨网关通信、gRPC 逻辑服路由，自带完整 MMORPG 游戏逻辑与客户端无关的协议契约层。

**当前版本**: v0.8.0 — 四层框架重构（协议补全 + 配置数据层 + 事件总线 + 领域拆分）

> 🎮 启动 `bevy-client`（Bevy 0.14 原生客户端）即可体验完整的 MMORPG 游戏！

---

## 目录

- [v0.8 更新亮点](#v08-更新亮点)
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

## v0.8 更新亮点

### 1. 四层框架重构（客户端无关基石）

v0.8 将网关 + 逻辑服 + 客户端彻底分层，服务端不再耦合任何特定客户端形态：

| 层 | 职责 | 实现 |
|----|------|------|
| **协议层** | 单一真相源 `proto/game.proto`，prost 自动生成 Rust SDK | 35 消息 + 1 枚举 + 包装器，5 个 TDD 测试 |
| **配置数据层** | 9 个 JSON 配置文件 + 热加载 + msg_id=101/9100 拉取协议 | `config/*.json` + `config_loader.rs` |
| **事件总线** | `GameEvent` + `SideEffect` + 3 订阅者，解耦业务逻辑 | `event_bus.rs`，handle_attack 已解耦 |
| **领域拆分** | handlers.rs 拆分为 5 个领域文件 | combat / inventory / quest / world / handlers |

### 2. Bevy 原生客户端（替代 HTML 客户端）

- 移除 `web-client/`（28 文件 / 9029 行 JS 代码）
- 新增 `bevy-client/`（Bevy 0.14.2 原生 Rust 客户端），8 个核心源文件：
  - `main.rs` — 应用入口 + 相机配置
  - `network.rs` — TCP 直连网关 7888 + AES-256-GCM 加密 + 读写任务分离
  - `codec.rs` — 14 上行编码 + 13 下行解码（protobuf）
  - `crypto.rs` — AES-GCM 加解密 + CRC32
  - `components.rs` — ECS 组件（Player/GameEntity/HealthBar/NameTag 等）
  - `resources.rs` — ECS 资源（PlayerState/EntityManager/Inventory 等）
  - `systems.rs` — 20+ 系统（渲染 / 输入 / 相机 / 插值 / 飘字 / 诊断）
  - `ui.rs` — HUD + 背包 + 任务 + 战斗日志 + NPC 对话面板

### 3. 网络层统一为 TCP 直连

- 移除 `ws_listener.rs`（网关 WebSocket 监听器）
- 客户端统一使用 `tokio::net::TcpStream::connect("127.0.0.1:7888")`
- 通道架构：`tokio::sync::mpsc`（命令通道，网络线程 await）+ `crossbeam`（事件通道，Bevy 主线程 try_recv）
- 读写任务分离：`ReadHalf` 用 `read_exact`（不可被 select! 取消），`WriteHalf` 在主循环

### 4. 服务端 9002 协议 protobuf 化

- `MobEntity` / `NpcEntity` 新增 `to_entity_list_entry()` 方法
- 4002 响应 + 登录 9002 广播全部使用 `dm_proto` 编码
- 修复客户端收不到实体列表的协议不匹配问题

### 5. Bevy 客户端可见性修复

- 相机 `OrthographicProjection.near = -1000.0`，确保 z>0 的 2D 实体不被近平面剔除
- 实体添加 `NoFrustumCulling` 组件，禁用视锥体剔除
- 新增 `visibility_diagnostic_system`，在 `CheckVisibility` 之后输出 `ViewVisibility` / `InheritedVisibility` 状态

---

## 架构概览

```
                    ┌─────────────┐
                    │   Client    │  (Bevy 原生客户端 / TCP :7888)
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

**13 个核心模块**（v0.8 移除 ws_listener）：

| 模块 | 职责 | 关键技术 |
|------|------|----------|
| `config` | 环境变量配置加载 | dotenv, APP_ENV 多环境 |
| `foundation` | 雪花ID生成器 | 无锁 AtomicU64 |
| `crypto` | AES-256-GCM 加密 | 12B nonce + 16B tag |
| `protocol` | 16B 定长包头 + CRC32 | 变长包体流式解码 |
| `network` | TCP 接入 + 握手里程碑 | Tokio TCP split |
| `session` | 会话管理（顶号/心跳） | DashMap 双映射 |
| `io_engine` | 小包合并 + 优先级队列 | 16ms 合并窗口, 泛型 ReadLoop/WriteLoop |
| `grpc_router` | gRPC 连接池 + 负载均衡 | tonic, 一致性哈希 |
| `cluster` | Redis 路由索引 + PubSub | 跨节点消息精准投递 |
| `security` | IP 黑名单 + 限流 + 审计 | 滑动窗口 + IP 前缀 |
| `admin` | HTTP 监控 + Prometheus | actix-web, 优雅停机 |
| `chat` | 聊天系统 | broadcast 广播 + 私聊/公会频道 |
| `combat/scene` | 战斗/场景/AOI/反外挂 | logic-lib 独立 crate |

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
| **配置热拉取** | **9 JSON 配置 + msg_id=101/9100 拉取协议** | **v0.8** |

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

# 2. 编译网关 (release + native 优化)
cargo build --release

# 3. 编译逻辑服
cd logic-lib && cargo build --release --bin logic-server

# 4. 编译 Bevy 客户端
cd ../bevy-client && cargo build --release

# 5. 启动逻辑服
cd ../logic-lib && cargo run --release --bin logic-server &

# 6. 启动网关 (TCP :7888)
cp .env.dev .env
cargo run --release

# 7. 启动 Bevy 客户端
cd bevy-client && cargo run --release
```

### 单节点 + 集群模式

```bash
# 单节点（默认）
./target/release/rust-mmo-gate.exe

# 双节点集群（Gate-2 用不同端口）
GATE_TCP_PORT=7889 GATE_HTTP_PORT=9091 GATE_NODE_ID=2 \
GATE_NODE_NAME=gate-dev-02 \
./target/release/rust-mmo-gate.exe
```

### Bevy 客户端操作

| 按键 | 功能 |
|------|------|
| WASD | 角色移动（带节流） |
| 鼠标左键 | 攻击 / NPC 交互 / 拾取 |
| 鼠标滚轮 | 相机缩放 |
| I | 背包面板 |
| Q | 任务面板 |
| L | 战斗日志 |
| 1-5 | NPC 对话选项 |
| R | 死亡后复活 |

---

## 生产部署

使用 Docker Compose 一键部署完整生产环境：

```bash
# 生产环境部署（3 网关 + Redis Sentinel + PG + Nginx + 监控）
cp .env.prod.example .env.prod  # 编辑密钥
docker compose -f docker-compose.prod.yml up -d

# 服务列表：
# - rust-mmo-gate ×3 (TCP 7888-7890)
# - Redis Sentinel: 1 主 + 2 从 + 3 哨兵
# - PostgreSQL 16
# - Nginx (L4 TCP 负载均衡)
# - Prometheus + Grafana (端口 3000)
# - Alertmanager (企业微信/钉钉告警)
```

---

## 客户端无关框架

v0.8 将四层框架彻底分离，服务端与客户端完全解耦：

```
proto/game.proto (单一真相源)
       │
       ├─→ Rust:    build.rs → prost 自动生成 → logic_server 使用
       ├─→ Bevy:    复用 rust-mmo-gate crate 的 game_proto 模块
       ├─→ Unity:   protoc --csharp_out → C# SDK（待生成）
       └─→ Godot:   protoc → GDScript（待生成）

config/*.json (配置数据层)
       │
       └─→ 客户端通过 msg_id=101 拉取所有配置 (items/quests/mobs/...)
```

### 配置数据层

9 个 JSON 配置文件作为服务端单一真相源：

| 文件 | 内容 |
|------|------|
| `items.json` | 物品定义（名称/类型/属性/图标） |
| `quests.json` | 任务定义（目标/奖励/前置） |
| `mobs.json` | 怪物定义（HP/攻击/掉落表） |
| `npcs.json` | NPC 定义（位置/对话/功能） |
| `maps.json` | 地图定义（传送门/区域） |
| `skills.json` | 技能定义（伤害/冷却/消耗） |
| `classes.json` | 职业定义（初始属性/天赋） |
| `talents.json` | 天赋定义（效果/层级） |
| `shop_items.json` | 商店商品定义 |

### 协议层使用

**Rust 端**（logic-server 已集成）：
```rust
use logic_lib::game_proto::PlayerStats;
use prost::Message;

let stats = PlayerStats { uid: 12345, name: "玩家".into(), hp: 100, max_hp: 100, ... };
let buf = stats.encode_to_vec();
let decoded = PlayerStats::decode(&buf[..]).unwrap();
```

**Bevy 客户端**（复用同一 proto）：
```rust
use rust_mmo_gate::game_proto::EntityList;

let list = EntityList::decode(&payload[..])?;
for mob in &list.mobs { /* 渲染怪物 */ }
```

### 事件总线

v0.8 引入事件总线解耦业务逻辑：

```rust
// handle_attack 只负责状态修改，通过事件总线发布副作用
let event = GameEvent::MobKilled { entity_id, killer_uid };
event_bus.publish(event);

// 订阅者独立处理: 掉落生成 / 广播 / 任务进度
```

### 测试验证

```bash
# Rust 协议层 TDD 测试（5 个）
cd logic-lib && cargo test --test tdd_proto

# logic-server 业务测试（含锁竞争 + BDD 场景）
cd logic-lib && cargo test --bin logic-server
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
| **logic-server (v0.8)** | - | ✅ 56 | ✅ | - | ✅ |

**总计**：16 个 TDD 套件 + logic-server 56 个内联测试，共 100+ 测试用例

### 测试运行

```bash
# 全部单元测试
cargo test --lib

# v0.8 协议层测试
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
├── src/                          # 网关核心代码 (13 模块)
│   ├── main.rs                   # 启动入口 (TCP 监听)
│   ├── config/                   # 配置管理
│   ├── foundation/               # 雪花 ID 生成器
│   ├── crypto/                   # AES-256-GCM 加密
│   ├── protocol/                 # 16B 包头 + CRC32
│   ├── network/                  # TCP 接入 + 握手里程碑
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
│       └── bin/logic_server/     # 主逻辑服 (v0.8 四层架构)
│           ├── main.rs           # 入口 + gRPC impl
│           ├── constants.rs      # 常量 + 静态数据
│           ├── types.rs          # 实体结构
│           ├── utils.rs          # 工具函数
│           ├── state.rs          # GameState + MockLogicService
│           ├── handlers.rs       # 通用业务方法
│           ├── combat.rs         # 战斗领域 (v0.8 拆分)
│           ├── inventory.rs      # 背包/装备/商店领域 (v0.8 拆分)
│           ├── quest.rs          # 任务领域 (v0.8 拆分)
│           ├── world.rs          # NPC/怪物 AI 领域 (v0.8 拆分)
│           ├── codec.rs          # proto 编解码
│           ├── config_loader.rs  # 配置数据层加载 (v0.8 新增)
│           ├── event_bus.rs      # 事件总线 (v0.8 新增)
│           └── tests.rs          # 56 个测试
│
├── bevy-client/                  # Bevy 原生客户端 (v0.8 新增)
│   ├── src/
│   │   ├── main.rs               # 应用入口 + 相机配置
│   │   ├── network.rs            # TCP 直连 + AES-GCM + 读写任务分离
│   │   ├── codec.rs              # 14 上行编码 + 13 下行解码
│   │   ├── crypto.rs             # AES-GCM + CRC32
│   │   ├── components.rs         # ECS 组件
│   │   ├── resources.rs          # ECS 资源
│   │   ├── systems.rs            # 20+ 系统 (渲染/输入/相机/诊断)
│   │   └── ui.rs                 # HUD + 面板
│   └── assets/fonts/simhei.ttf   # 中文字体
│
├── config/                       # 配置数据层 (v0.8 新增)
│   ├── items.json                # 物品配置
│   ├── quests.json               # 任务配置
│   ├── mobs.json                 # 怪物配置
│   ├── npcs.json                 # NPC 配置
│   ├── maps.json                 # 地图配置
│   ├── skills.json               # 技能配置
│   ├── classes.json              # 职业配置
│   ├── talents.json              # 天赋配置
│   └── shop_items.json           # 商店配置
│
├── proto/                        # Protobuf 协议定义
│   ├── gate.proto                # 网关 gRPC 协议
│   └── game.proto                # 游戏消息协议 (35 消息)
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
│   ├── nginx/                    # Nginx L4 TCP
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
| 协议 | 16B 定长包头 + CRC32 + 变长包体 (TCP) |
| 序列化 | prost (Protobuf) + serde_json |
| gRPC | tonic 0.12 |
| HTTP | actix-web 4 |
| Redis | redis-rs (连接池 + PubSub + Sentinel) |
| 数据库 | PostgreSQL + SQLite (离线降级) |
| 游戏引擎 | Bevy 0.14.2 (ECS + 渲染 + 输入) |
| 监控 | Prometheus + Grafana + Alertmanager |
| 部署 | Docker Compose + Nginx L4 |
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
| v0.7 | 框架化 | Protobuf 协议层 / 模块化重构 / 稳定性修复 / 装备强化 |
| **v0.8** | **四层架构** | **协议补全 + 配置数据层 + 事件总线 + 领域拆分 + Bevy 客户端 + TCP 统一** |

详细路线图见 [ROADMAP.md](ROADMAP.md)

---

## 许可证

MIT License

---

*最后更新: 2026-07-13 | v0.8.0 四层框架重构 | 80K pps 吞吐 | 100+ 测试通过 | Bevy 原生客户端*
