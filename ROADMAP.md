# Rust MMO Gateway — 项目路线图

> **当前版本**: v0.3.0 | **最后更新**: 2026-07-11 | **仓库**: [github.com/HavocZhang/mmorpg](https://github.com/HavocZhang/mmorpg)

---

## 项目概述

Rust 实现的 MMO 百万在线网关集群 + MMORPG 游戏逻辑服 + 网页客户端原型。

```
浏览器(game.html) → WebSocket(9000) → ws_proxy.js → TCP(7888) → Gateway → gRPC → LogicServer
                                                          ↕ Redis PubSub (集群)
                                                    PostgreSQL (持久化)
```

---

## 已完成功能

### 网络网关 (src/)

| 模块 | 功能 | 技术 |
|------|------|------|
| protocol | 16 字节定长包头 + CRC32 校验 + AES-256-GCM 加密 | 随机 nonce |
| network | TCP 接入, 读写分离, 握手认证, Token 验证 ≥8 字符 | Tokio async |
| session | DashMap 会话管理, 在线统计, 心跳保活 | parking_lot |
| grpc_router | gRPC 连接池, upstream 路由, downstream 分发 | Tonic |
| cluster | Redis PubSub 跨节点广播, 自忽略机制, 路由索引 | redis-rs |
| security | 速率限制, IP 黑名单, 连接数限制 | token bucket |
| admin | RESTful 健康检查, 在线统计, 监控指标 | axum |
| benchmark | throughput_bench 吞吐压测工具 | 200 连接 80K pps |

### 游戏逻辑服 (logic-lib/)

| 系统 | 消息 ID | 功能描述 |
|------|---------|----------|
| 战斗 | 1001/1002 → 6001/6002 | 普攻+技能, 暴击判定, 伤害计算, 经验获取 |
| 怪物 AI | 内部 200ms tick | 巡逻/追击/攻击/死亡复活, 位置 8004 广播 |
| 聊天 | 2001 → 7001/7002 | 世界频道/队伍频道, 广播+ACK |
| 任务 | 1005/1006 → 5005 | 接取/进度追踪/提交, 5 个任务, 物品奖励 |
| 背包 | 5003 | 物品堆叠, 使用/装备, 10 种物品 |
| 装备 | 1004 → 5004 | 武器/护甲/饰品 3 槽, 属性加成自动计算 |
| 掉落 | 1003 → 6003 | 5 种怪物掉落表, 拾取距离 60 单位 |
| 组队 | 2002/2003/2004 | 邀请/接受/离开, DashMap 内存 |
| NPC | 1005 → 5006 | 对话弹窗, 任务接取/提交 |
| 持久化 | 内部 30s 定时 | PostgreSQL sqlx, DB 不可用内存降级 |

### 网页客户端 (web-client/game.html v5.9)

| 功能 | 操作方式 |
|------|----------|
| 移动 | WASD / 方向键, 平滑相机跟随, 世界边界 1600×1200 |
| 攻击 | 1 普攻(自动锁定最近目标), 2-5 技能, 0.8s/2s CD |
| 药水 | Q 生命药水(+50HP), E 法力药水(+30MP) |
| NPC 对话 | F 键, 弹出对话窗 → 任务接取/提交 |
| 拾取 | G 键, 自动拾取最近掉落物 |
| 组队 | T 键, 接受对方邀请 |
| 聊天 | Enter 打开聊天框, 世界/队伍频道 |
| 装备 | 背包内点击「装备」, 3 槽显示绿色=已装备 |
| 任务 | 右侧面板实时进度, NPC 提交 |
| 攻击力 | 显示基础值+装备加成, 如 `20 (+15)` |

### 测试

| 套件 | 测试数 | 说明 |
|------|--------|------|
| Rust TDD (11 suites) | 87 | 协议/加密/配置/会话/网络/IO/安全/集群/管理/异常/模糊 |
| tdd_concurrent | 7 | 并发安全: session/DashMap/Atomic/gRPC/IO 引擎 |
| logic-lib unit | 81 | 战斗/聊天/任务/场景 |
| BDD cucumber | 73/74 | Gherkin 场景, 1 跳过(离线消息语义) |
| E2E (7 suites) | 45 | game/monster_ai/quest/backpack/multiplayer/persist/party |
| **总计** | **440+** | **0 失败** |

### 性能指标

| 场景 | 指标 | 结果 |
|------|------|------|
| 吞吐量 (Rust bench) | 200 连接, 10s | **80,862 pps** |
| 吞吐量 (Node.js) | 100 连接, 单进程 | 60,120 pps |
| 合并压缩 | 6 条消息→1 包 | 压缩率 73.37% |
| 稳定性 | 2500 连接 56min | 0 WARN, 0 内存泄漏 |
| cargo audit | 依赖安全扫描 | 0 漏洞 |

---

## 路线图

### v0.4 — 客户端可玩 (预计 2 周)

- [ ] **协议文档** — 所有 msg_id 枚举 + 字段定义 + 示例 JSON
- [ ] **E2E 全覆盖** — 每条 msg_id 至少 1 个自动化收发测试
- [ ] **NPC 任务列表服务端下发** — 9002 携带 NPC 任务数据(不再硬编码)
- [ ] **装备实时反映战斗力** — 5004 装备消息即时刷新攻击/防御
- [ ] **技能粒子特效** — Canvas 攻击/技能动画
- [ ] **装备对比** — 装备前显示新旧属性差异
- [ ] **背包排序/整理** — 物品按类型/稀有度排序

### v0.5 — 服务器生产化 (预计 3 周)

- [ ] **Linux Docker Compose 生产部署** — 3 gate + Redis Sentinel + PG + Prometheus
- [ ] **WebSocket 原生支持** — Gateway 直连 WS, 去掉 ws_proxy.js
- [ ] **Grafana 监控面板** — QPS/延迟/在线/内存/错误率
- [ ] **告警通知** — 企业微信/钉钉 Webhook
- [ ] **正式压测报告** — 1 万并发 × 72h, 趋势图表
- [ ] **GitHub Actions CI/CD** — push → clippy → test → bench → docker build
- [ ] **反外挂基础** — 移动速度/攻击频率/背包校验
- [ ] **SQLite 离线模式** — PostgreSQL 不可用时降级 SQLite

### v0.6 — 游戏内容 (预计 4 周)

- [ ] **新地图** — 森林/沙漠/地下城, 传送门
- [ ] **Boss 副本** — 多人副本, Boss 技能阶段, 掉落池
- [ ] **经济系统** — NPC 商店, 金币交易, 物品买卖
- [ ] **公会系统** — 创建/加入/公会仓库/公会战
- [ ] **好友/私聊** — 好友列表, 私聊频道, 在线状态
- [ ] **技能树** — 职业系统, 天赋加点
- [ ] **PvP 竞技场** — 1v1/3v3, 段位系统

### v0.7+ — 运营化

- [ ] 移动端客户端 (React Native / Flutter)
- [ ] Web 后台管理面板 (GM 工具)
- [ ] 热更新/灰度发布
- [ ] 跨服战场
- [ ] 排行榜系统

---

## 架构图

```
                    ┌─────────────┐
                    │  Browser     │  Web 客户端
                    │  game.html   │
                    └──────┬──────┘
                           │ WebSocket :9000
                    ┌──────▼──────┐
                    │  ws_proxy   │  WS↔TCP 桥接
                    │  (Node.js)  │
                    └──────┬──────┘
                           │ TCP :7888
            ┌──────────────┼──────────────┐
            │              │              │
     ┌──────▼──────┐ ┌────▼─────┐ ┌─────▼─────┐
     │   Gate-1    │ │  Gate-2  │ │  Gate-N   │  网关集群
     │  :7888/9090 │ │ :7889/   │ │  ...      │
     └──────┬──────┘ └────┬─────┘ └─────┬─────┘
            │              │              │
            └──────────────┼──────────────┘
                           │ gRPC :50051
                    ┌──────▼──────┐
                    │ LogicServer │  游戏逻辑
                    │  (tokio)    │
                    └──┬──────┬───┘
                       │      │
              ┌────────▼─┐  ┌─▼──────────┐
              │   Redis   │  │ PostgreSQL │
              │  PubSub   │  │  持久化     │
              └──────────┘  └────────────┘
```

---

## 快速开始

### 开发环境

```bash
# 前置条件: Rust 1.95+, Docker, Node.js 22+

# 1. 启动依赖服务
docker run -d --name redis-mmo -p 6379:6379 redis:7-alpine
docker run -d --name mmo-pg -p 5433:5432 \
  -e POSTGRES_DB=mmorpg -e POSTGRES_USER=mmo -e POSTGRES_PASSWORD=mmo_dev_pass \
  postgres:16-alpine
# 初始化 DB schema
docker exec -i mmo-pg psql -U mmo -d mmorpg < deploy/postgres/init.sql

# 2. 编译
cargo build --release
cd logic-lib && cargo build --release --bin logic-server && cd ..

# 3. 一键启动
# Gateway + LogicServer + WS Proxy + HTTP Server
# (手动启动序列见 start_all.sh)

# 4. 打开浏览器
# http://localhost:4000/game.html
```

### 运行测试

```bash
# 全部自动化测试
bash ci.sh                          # Rust TDD + clippy
cd logic-lib && cargo test && cd .. # 逻辑服测试
cd web-client
for f in test_game_e2e test_e2e_monster_ai test_e2e_quest \
         test_e2e_backpack test_e2e_multiplayer test_e2e_persist \
         test_e2e_party; do node "$f.js"; done
```

### 生产部署

```bash
# Docker Compose 完整部署
docker compose -f docker-compose.yml -f docker-compose.monitoring.yml up -d
```

---

## 协议消息一览

### 上行 (Client → Server)

| msg_id | 名称 | 参数 |
|--------|------|------|
| 1 | 握手 | `{uid, token, version, timestamp}` |
| 1001 | 普攻 | `{skillId:1, targetUid}` |
| 1002 | 技能攻击 | `{skillId, targetUid}` |
| 1003 | 拾取掉落 | `{dropId}` |
| 1004 | 装备/卸下 | `{itemId, slot}` |
| 1005 | 接任务 | `{questId}` |
| 1006 | 交任务 | `{questId}` |
| 1008 | 使用物品 | `{itemId}` |
| 2001 | 聊天 | `{from, text, channel}` |
| 2002 | 组队邀请 | `{targetUid}` |
| 2003 | 接受组队 | `{inviterUid}` |
| 2004 | 离开队伍 | `{}` |
| 3001 | 移动 | `{x, y, dir}` |
| 4001 | 心跳 | `{}` |
| 4002 | 实体查询 | `{}` |

### 下行 (Server → Client)

| msg_id | 名称 | 内容 |
|--------|------|------|
| 5001 | 玩家属性 | `{uid, name, hp, maxHp, mp, maxMp, level, exp, maxExp, x, y, atk, def}` |
| 5002 | 经验/MP 更新 | `{exp, maxExp, level, gained}` 或 `{mp, maxMp}` |
| 5003 | 背包 | `{items: [{itemId, count, name, type, icon}]}` |
| 5004 | 装备 | `{weapon: {itemId,name,icon}\|null, armor, accessory}` |
| 5005 | 任务 | `{quests: [{questId, name, progress, target}]}` |
| 5006 | NPC 对话 | `{npcId, name, dialog, options}` |
| 6001 | 战斗结果 | `{targetUid, damage, targetHp, crit, miss, playerHp, expGained}` |
| 6002 | 实体状态 | `{entityId, hp, maxHp, state, x, y}` |
| 6003 | 掉落 | `{entityId, killer, mobName, drops: [{dropId,itemId,count,x,y}], exp}` |
| 7001 | 聊天 ACK | `{status, messageId}` |
| 7002 | 聊天广播 | `{from, fromName, text, channel, type}` |
| 8001 | 位置更新 | `{uid, x, y, dir}` |
| 8002 | 玩家进入 | `{uid, name, x, y}` |
| 8003 | 玩家离开 | `{uid}` |
| 8004 | 实体位置 | `{entityId, x, y, hp, maxHp}` |
| 9001 | 玩家列表 | `{players: [{uid, name, x, y}]}` |
| 9002 | 实体列表 | `{npcs: [JSON strings], mobs: [JSON strings]}` |

---

## 物品系统

### 物品清单 (10 种)

| ID | 名称 | 类型 | 属性 | 价值 |
|----|------|------|------|------|
| 1 | 铁剑 | 武器 | ATK+15 | 100 |
| 2 | 钢剑 | 武器 | ATK+30 | 300 |
| 3 | 皮甲 | 护甲 | DEF+10 | 150 |
| 4 | 铁甲 | 护甲 | DEF+25 | 400 |
| 5 | 力量戒指 | 饰品 | ATK+10 DEF+5 | 200 |
| 6 | 生命药水 | 药水 | HP+50 | 50 |
| 7 | 法力药水 | 药水 | MP+30 | 50 |
| 8 | 全恢复药水 | 药水 | HP+100 MP+50 | 150 |
| 9 | 史莱姆凝胶 | 材料 | — | 10 |
| 10 | 哥布林耳朵 | 材料 | — | 15 |

### 怪物掉落

| 怪物 | 必掉 | 概率掉落 |
|------|------|----------|
| 史莱姆 | 凝胶(9)×1 | 生命药水(6) 33% |
| 哥布林 | 耳朵(10)×1 | 法力药水(7) 50% |
| 骷髅战士 | 铁剑(1)×1 | 生命药水(6) 50% |
| 暗影法师 | 钢剑(2)+全恢复(8) | — |
| 岩石巨人 | 铁甲(4)+戒指(5)+全恢复(8) | — |

---

## 项目结构

```
rust-mmo-gate/
├── src/                          # 网关核心
│   ├── config/                   # 配置管理
│   ├── foundation/               # 基础工具
│   ├── crypto/                   # AES-256-GCM 加密
│   ├── protocol/                 # 16B包头协议
│   ├── network/                  # TCP 接入, 读写分离
│   ├── session/                  # 会话管理
│   ├── io_engine/                # IO 引擎
│   ├── grpc_router/              # gRPC 路由
│   ├── cluster/                  # Redis PubSub 集群
│   ├── security/                 # 安全/限流
│   ├── admin/                    # 管理 API
│   └── main.rs                   # 入口
├── logic-lib/                    # 游戏逻辑库
│   ├── src/
│   │   ├── scene/                # 场景/AOI
│   │   ├── chat/                 # 聊天
│   │   ├── combat/               # 战斗
│   │   ├── db.rs                 # PostgreSQL 持久化
│   │   ├── party.rs              # 组队系统
│   │   └── bin/logic_server.rs   # 逻辑服(14KB 单体)
│   └── tests/tdd_unit/           # 逻辑服单元测试
├── web-client/                   # 网页客户端
│   ├── game.html                 # 游戏主页面 (~500行)
│   ├── ws_proxy.js               # WebSocket↔TCP 代理
│   ├── sdk/
│   │   ├── game_sdk.js           # 浏览器协议 SDK
│   │   └── items.js              # 物品数据层
│   ├── test_game_e2e.js          # 全链路 E2E (13 tests)
│   ├── test_e2e_*.js             # 专项 E2E (6 套 × 32 tests)
│   └── test_stability_v2.js      # 长稳压力测试
├── tests/
│   ├── tdd_*/                    # TDD 套件 (11 套, 87 tests)
│   ├── bdd/                      # BDD cucumber (74 scenarios)
│   └── bdd_feature/              # Gherkin feature 文件
├── deploy/
│   └── postgres/init.sql         # 数据库初始化
├── docker-compose.yml            # Docker 编排
├── docker-compose.monitoring.yml # Prometheus + Grafana
├── Dockerfile                    # 多阶段构建
├── ci.sh                         # CI 流水线脚本
├── start_all.sh                  # 一键启动全栈
├── ROADMAP.md                    # 本文档
└── README.md                     # 项目介绍
```

---

## 变更日志

### v0.3.0 (2026-07-11)
- PostgreSQL 持久化 + 30s 自动存盘
- 组队系统 (邀请/接受/离开)
- 怪物 AI 独立后台循环 (200ms tick)
- 物品系统完善 (10 种物品, 装备面板 3 槽)
- 网页客户端 v5.9 (12 项 bug 修复)
- 7 套 E2E 测试 (45 tests)

### v0.2.0 (2026-07-11)
- 集群多节点跨网关消息 (Redis PubSub)
- 安全审计 (cargo audit 0 漏洞)
- CI 流水线脚本
- 架构迁移 logic-lib 独立 crate
- 80K pps 吞吐达标
- 日志洪水修复 (6 处 → 0 WARN)

### v0.1.0 (2026-07-11)
- TCP 接入, 16B 包头协议, AES-256-GCM 加密
- 会话管理 (DashMap + parking_lot)
- gRPC 路由 + 下游分发
- 13 个 TDD 套件
- 初始架构 13 模块
