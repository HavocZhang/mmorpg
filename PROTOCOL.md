# Rust MMO Gateway — 协议文档

> 版本: v0.4 | 最后更新: 2026-07-11

---

## 概述

本协议运行在 TCP 长连接之上，采用**16 字节定长包头 + 变长包体**的二进制帧格式，包体使用 **AES-256-GCM** 加密，并通过 **CRC32** 校验完整性。

### 传输层

```
客户端 ←→ TCP(7888) ←→ Gateway ←→ gRPC(50051) ←→ LogicServer
```

- 客户端通过 WebSocket 代理（`ws_proxy.js` 监听 9000）转接到 TCP
- Gateway 与 LogicServer 之间通过 gRPC 通信

---

## 二进制帧格式

```
偏移  大小  字段        说明
────────────────────────────────
 0     2    Magic       固定值 0x4D 0x4D ("MM")
 2     1    Version     协议版本，当前 = 1
 3     1    Reserved    保留
 4     2    MsgId       消息 ID (大端序 u16)
 6     2    BodyLen     加密后包体长度 (大端序 u16)
 8     4    CRC32       加密后包体的 CRC32 校验值
12     4    Flags       标志位 (预留)
────────────────────────────────
      16    HEADER_SIZE
```

### 包体编码

```
包体 (变长) = Nonce(12B) + Ciphertext(变长) + AuthTag(16B)
```

- 明文 = UTF-8 JSON 字符串
- 对称密钥: 32 字节 AES-256 密钥 (通过环境变量 `AES_KEY` 注入)
- Nonce: 每次加密随机生成 12 字节
- 最大包体: 8192 字节

### 粘包处理

解码器使用 `BytesMut` 缓冲区，按帧头读取，支持 TCP 粘包。

---

## 消息 ID 枚举

### 上行 (Client → Server)

| MsgId | 名称 | 描述 |
|--------|------|------|
| 1 | 握手认证 | 建立连接时的身份验证 |
| 100 | 初始化 | 请求玩家列表 |
| 1001 | 普攻 | 近战基础攻击 |
| 1002 | 技能攻击 | 释放指定技能 |
| 1003 | 拾取掉落 | 拾取地面掉落物 |
| 1004 | 装备物品 | 装备/卸下物品到指定槽位 |
| 1005 | 接取任务 | 接受 NPC 任务 |
| 1006 | 提交任务 | 提交已完成的任务 |
| 1007 | NPC 交互 | 与 NPC 对话 |
| 1008 | 使用物品 | 使用药水等消耗品 |
| 2001 | 聊天 | 发送聊天消息 |
| 2002 | 组队邀请 | 邀请玩家加入队伍 |
| 2003 | 接受组队 | 接受组队邀请 |
| 2004 | 离开队伍 | 离开当前队伍 |
| 3001 | 移动 | 更新玩家位置 |
| 4001 | 查询玩家 | 获取附近玩家列表 |
| 4002 | 查询实体 | 获取 NPC 和怪物列表 |

### 下行 (Server → Client)

| MsgId | 名称 | 描述 |
|--------|------|------|
| 5001 | 玩家属性 | 完整角色属性同步 |
| 5002 | 经验/MP 更新 | 经验值、等级或 MP 变化 |
| 5003 | 背包更新 | 背包物品列表 |
| 5004 | 装备更新 | 装备槽位状态 |
| 5005 | 任务更新 | 任务列表和进度 |
| 5006 | NPC 对话 | NPC 对话内容和选项 |
| 5500 | 技能列表 | 技能定义和冷却状态 |
| 6001 | 战斗结果 | 攻击伤害/暴击/闪避结果 |
| 6002 | 实体状态 | 怪物 HP/位置更新 |
| 6003 | 掉落/死亡 | 怪物死亡掉落或物品被拾取 |
| 7001 | 聊天 ACK | 消息已送达确认 |
| 7002 | 聊天广播 | 聊天消息广播 |
| 8001 | 位置更新 | 玩家移动位置同步 |
| 8002 | 玩家进入 | 新玩家进入视野 |
| 8003 | 玩家离开 | 玩家离线或离开 |
| 8004 | 实体位置 | 怪物位置和 HP 更新 |
| 9001 | 玩家列表 | 在线玩家列表 |
| 9002 | 实体列表 | NPC 和怪物完整列表 |

---

## 详细协议

### 1. 握手认证

**上行 MsgId = 1**

```json
{
  "uid": 12345,
  "token": "tok_abcdefgh",
  "version": 1,
  "timestamp": 1692000000
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| uid | u64 | 玩家 ID |
| token | string | 认证令牌, ≥8 字符 |
| version | u8 | 协议版本号 |
| timestamp | u64 | Unix 时间戳 (秒), 10 分钟内有效 |

**下行: 无直接响应** — 认证成功后会连续下发 5001/5003/5004/5005/9001/9002 等消息。

---

### 2. 玩家属性 (5001)

**下行 MsgId = 5001**

```json
{
  "uid": 12345,
  "name": "Player12345",
  "hp": 85,
  "maxHp": 100,
  "mp": 40,
  "maxMp": 50,
  "level": 3,
  "exp": 150,
  "maxExp": 900,
  "x": 420.0,
  "y": 350.0,
  "atk": 35,
  "def": 15
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| uid | u64 | 玩家 ID |
| name | string | 玩家名称 |
| hp | i32 | 当前生命值 |
| maxHp | i32 | 最大生命值 |
| mp | i32 | 当前法力值 |
| maxMp | i32 | 最大法力值 |
| level | u32 | 等级 (≥1) |
| exp | u32 | 当前经验值 |
| maxExp | u32 | 升级所需经验 |
| x, y | f32 | 当前位置坐标 |
| atk | i32 | 总攻击力 (含装备加成) |
| def | i32 | 总防御力 (含装备加成) |

**升级时额外发送**:
```json
{
  "level": 4,
  "maxHp": 120,
  "maxMp": 60,
  "hp": 120,
  "mp": 60,
  "atk": 40,
  "def": 17
}
```

**复活时**:
```json
{
  "hp": 100,
  "maxHp": 100,
  "mp": 50,
  "maxMp": 50,
  "revived": true
}
```

---

### 3. 经验/MP 更新 (5002)

**下行 MsgId = 5002**

经验获得:
```json
{
  "exp": 250,
  "maxExp": 900,
  "level": 3,
  "gained": 50
}
```

MP 变化:
```json
{
  "mp": 30,
  "maxMp": 50
}
```

---

### 4. 背包 (5003)

**下行 MsgId = 5003**

```json
{
  "items": [
    {"itemId": 6, "count": 3, "name": "生命药水", "type": "potion", "icon": "🧪"},
    {"itemId": 7, "count": 2, "name": "法力药水", "type": "potion", "icon": "💧"},
    {"itemId": 1, "count": 1, "name": "铁剑", "type": "weapon", "icon": "🗡"},
    {"itemId": 9, "count": 5, "name": "史莱姆凝胶", "type": "material", "icon": "🟢"}
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| items | array | 背包物品数组 |
| items[].itemId | u32 | 物品 ID (见物品表) |
| items[].count | u32 | 堆叠数量 |
| items[].name | string | 物品名称 |
| items[].type | string | 物品类型: weapon/armor/accessory/potion/material |
| items[].icon | string | 显示图标 (Emoji) |

---

### 5. 装备 (5004)

**下行 MsgId = 5004**

```json
{
  "weapon": {"itemId": 2, "name": "钢剑", "icon": "⚔"},
  "armor": {"itemId": 3, "name": "皮甲", "icon": "🛡"},
  "accessory": null
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| weapon | object\|null | 武器槽 (null = 空) |
| armor | object\|null | 护甲槽 (null = 空) |
| accessory | object\|null | 饰品槽 (null = 空) |
| {slot}.itemId | u32 | 物品 ID |
| {slot}.name | string | 物品名称 |
| {slot}.icon | string | 图标 |

**上行 MsgId = 1004**

装备物品:
```json
{"itemId": 2, "slot": "weapon"}
```

卸下物品 (itemId=0 或不发送 slot):
```json
{"itemId": 0}
```

---

### 6. 任务 (5005)

**下行 MsgId = 5005**

```json
{
  "quests": [
    {
      "questId": 1,
      "name": "清除史莱姆",
      "desc": "消灭5只史莱姆",
      "progress": 3,
      "target": 5,
      "completed": false
    }
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| quests | array | 已接取任务列表 |
| questId | u32 | 任务 ID |
| name | string | 任务名称 |
| desc | string | 任务描述 |
| progress | u32 | 当前进度 |
| target | u32 | 目标数量 |
| completed | bool | 是否已完成 (可提交) |

**上行 MsgId = 1005: 接取任务**
```json
{"questId": 1}
```

**上行 MsgId = 1006: 提交任务**
```json
{"questId": 1}
```

---

### 7. NPC 对话 (5006)

**下行 MsgId = 5006**

```json
{
  "npcId": 1,
  "name": "村长·李四",
  "dialog": "欢迎来到新手村！",
  "type": "quest_giver",
  "options": [
    {"type": "accept_quest", "questId": 1, "label": "接受任务: 清除史莱姆"},
    {"type": "complete_quest", "questId": 1, "label": "完成任务: 清除史莱姆"},
    {"type": "heal", "label": "完全恢复 (免费)"},
    {"type": "shop", "label": "查看商品"}
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| npcId | u32 | NPC ID |
| name | string | NPC 名称 |
| dialog | string | 对话文本 |
| type | string | NPC 类型: quest_giver/merchant/healer |
| options | array | 交互选项 |
| options[].type | string | 选项类型: accept_quest/complete_quest/heal/shop |
| options[].questId | u32? | 关联任务 ID (任务选项时) |
| options[].label | string | 显示文本 |

**上行 MsgId = 1007: NPC 交互**
```json
{"npcId": 1}
```

---

### 8. 技能列表 (5500)

**下行 MsgId = 5500**

```json
{
  "skills": [
    {"skillId": 1, "name": "普通攻击", "icon": "⚔", "mpCost": 0, "cooldownMs": 800, "cooldownLeft": 0, "range": 80.0},
    {"skillId": 2, "name": "重击",     "icon": "💥", "mpCost": 10, "cooldownMs": 2000, "cooldownLeft": 1200, "range": 80.0},
    {"skillId": 3, "name": "火球术",   "icon": "🔥", "mpCost": 20, "cooldownMs": 3000, "cooldownLeft": 0, "range": 200.0},
    {"skillId": 4, "name": "冰冻",     "icon": "❄", "mpCost": 15, "cooldownMs": 4000, "cooldownLeft": 0, "range": 150.0},
    {"skillId": 5, "name": "治疗术",   "icon": "💚", "mpCost": 25, "cooldownMs": 5000, "cooldownLeft": 0, "range": 0.0}
  ]
}
```

---

### 9. 战斗 (6001, 6002)

**下行 MsgId = 6001 — 战斗结果**

普攻命中:
```json
{
  "targetUid": 10001,
  "targetName": "史莱姆",
  "dmg": 25,
  "targetHp": 25,
  "crit": false,
  "skillId": 1
}
```

暴击:
```json
{
  "targetUid": 10001,
  "targetName": "史莱姆",
  "dmg": 50,
  "targetHp": 0,
  "crit": true,
  "skillId": 1
}
```

闪避/超出范围:
```json
{
  "targetUid": 10001,
  "dmg": 0,
  "targetHp": 50,
  "miss": true,
  "reason": "out_of_range"
}
```

冷却中:
```json
{
  "error": "cooldown",
  "skillId": 2,
  "cooldownLeft": 800
}
```

法力不足:
```json
{"error": "not_enough_mp"}
```

技能不存在:
```json
{"error": "invalid_skill"}
```

被玩家攻击时收到:
```json
{
  "attackerUid": 12345,
  "attackerName": "Player12345",
  "dmg": 18,
  "hp": 82,
  "maxHp": 100,
  "crit": false
}
```

**下行 MsgId = 6002 — 实体状态**
```json
{
  "entityId": 10001,
  "hp": 25,
  "maxHp": 50,
  "state": "Chasing",
  "x": 520.0,
  "y": 410.0
}
```

---

### 10. 掉落/死亡 (6003)

**下行 MsgId = 6003 — 怪物死亡掉落**
```json
{
  "entityId": 10001,
  "killer": 12345,
  "killerName": "Player12345",
  "mobName": "史莱姆",
  "drops": [
    {"dropId": 20001, "itemId": 9, "count": 1, "x": 510.0, "y": 420.0, "name": "史莱姆凝胶", "icon": "🟢"}
  ],
  "exp": 20
}
```

**下行 MsgId = 6003 — 物品被拾取**
```json
{
  "dropId": 20001,
  "pickedBy": 12345
}
```

**上行 MsgId = 1003: 拾取**
```json
{"dropId": 20001}
```

---

### 11. 攻击 (1001, 1002)

**上行 MsgId = 1001: 普攻**
```json
{"skillId": 1, "targetUid": 10001}
```

**上行 MsgId = 1002: 技能攻击**
```json
{"skillId": 3, "targetUid": 10001}
```

伤害计算公式:
```
最终伤害 = max(1, floor(攻击力 × 技能倍率 - 目标防御 × 0.5))
暴击伤害 = 最终伤害 × 2
暴击率 = 20%
```

---

### 12. 聊天 (2001, 7001, 7002)

**上行 MsgId = 2001**
```json
{
  "from": 12345,
  "text": "大家好!",
  "channel": "world"
}
```

**下行 MsgId = 7001 — ACK**
```json
{"msgId": 2001}
```

**下行 MsgId = 7002 — 广播**
```json
{
  "from": 12345,
  "fromName": "Player12345",
  "text": "大家好!"
}
```

系统消息:
```json
{
  "from": 0,
  "fromName": "System",
  "text": "Player12345 升到了 4 级!"
}
```

---

### 13. 组队 (2002~2004, 7001, 7002)

**上行 MsgId = 2002: 邀请**
```json
{"targetUid": 12346}
```

**下行: 被邀请者收到**
```json
{
  "type": "party_invite",
  "from": 12345,
  "fromName": "Player12345",
  "partyId": 1
}
```

**上行 MsgId = 2003: 接受邀请**
```json
{}
```

**下行: 创建者收到 ACK**
```json
{"type": "party_created", "partyId": 1}
```

**下行: 加入者收到 ACK**
```json
{"type": "party_joined", "partyId": 1}
```

**下行: 广播加入**
```json
{
  "type": "party_join",
  "uid": 12346,
  "name": "Player12346",
  "partyId": 1
}
```

**上行 MsgId = 2004: 离开**
```json
{}
```

**下行: 广播离开**
```json
{
  "type": "party_leave",
  "uid": 12346,
  "name": "Player12346"
}
```

---

### 14. 移动 (3001 → 8001)

**上行 MsgId = 3001**
```json
{"x": 420.0, "y": 350.0, "dir": 1}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| x, y | f32 | 目标位置 (0~1600, 0~1200) |
| dir | u8 | 方向 (1=右 3=左) |

**下行 MsgId = 8001 — 广播位置**
```json
{"uid": 12345, "x": 420.0, "y": 350.0, "dir": 1}
```

---

### 15. 实体查询 (4001 → 9001, 4002 → 9002)

**上行 MsgId = 4001: 查询玩家**
```json
{}
```

**下行 MsgId = 9001**
```json
{
  "players": [
    {"uid": 12346, "name": "Player12346", "x": 500.0, "y": 400.0, "hp": 100, "maxHp": 100, "level": 2}
  ]
}
```

**上行 MsgId = 4002: 查询实体**
```json
{}
```

**下行 MsgId = 9002**
```json
{
  "npcs": [
    "{\"id\":1,\"name\":\"村长·李四\",\"x\":200.0,\"y\":200.0,\"type\":\"quest_giver\",\"dialog\":\"欢迎来到新手村！\"}",
    "{\"id\":2,\"name\":\"商人·王五\",\"x\":1400.0,\"y\":200.0,\"type\":\"merchant\",\"dialog\":\"各种药水、装备应有尽有！\"}"
  ],
  "mobs": [
    "{\"entityId\":10001,\"defId\":1,\"name\":\"史莱姆\",\"x\":500.0,\"y\":400.0,\"hp\":50,\"maxHp\":50,\"level\":1,\"state\":\"Idle\"}"
  ]
}
```

> **注意**: npcs 和 mobs 的元素是 JSON 字符串（需 `JSON.parse()`），不是 JSON 对象。

自 v0.4 起，9002 的 NPC 数据**包含可用任务列表**:

```json
{
  "npcs": [
    "{\"id\":1,\"name\":\"村长·李四\",\"x\":200.0,\"y\":200.0,\"type\":\"quest_giver\",\"dialog\":\"欢迎来到新手村！\",\"quests\":[1,2,3,4,5]}"
  ]
}
```

`quests` 字段包含该 NPC 可提供/可提交的任务 ID 列表。

---

### 16. 物品使用 (1008)

**上行 MsgId = 1008**
```json
{"itemId": 6}
```

**响应: 更新 5003 (背包) + 5001 (属性)**

---

### 17. 玩家进入/离开 (8002, 8003)

**下行 MsgId = 8002 — 玩家进入**
```json
{
  "uid": 12346,
  "name": "Player12346",
  "x": 500.0,
  "y": 400.0,
  "hp": 100,
  "maxHp": 100,
  "level": 2
}
```

**下行 MsgId = 8003 — 玩家离开**
```json
{"uid": 12346}
```

---

### 18. 实体位置 (8004)

**下行 MsgId = 8004 — 怪物位置广播**
```json
{
  "entityId": 10001,
  "x": 520.0,
  "y": 410.0,
  "hp": 45,
  "maxHp": 50
}
```

---

## 物品表

| ID | 名称 | 类型 | 图标 | ATK | DEF | HP | MP | 价值 |
|----|------|------|------|-----|-----|----|----|------|
| 1 | 铁剑 | weapon | 🗡 | +15 | 0 | 0 | 0 | 100 |
| 2 | 钢剑 | weapon | ⚔ | +30 | 0 | 0 | 0 | 300 |
| 3 | 皮甲 | armor | 🛡 | 0 | +10 | 0 | 0 | 150 |
| 4 | 铁甲 | armor | 🛡 | 0 | +25 | 0 | 0 | 400 |
| 5 | 力量戒指 | accessory | 💍 | +10 | +5 | 0 | 0 | 200 |
| 6 | 生命药水 | potion | 🧪 | 0 | 0 | +50 | 0 | 50 |
| 7 | 法力药水 | potion | 💧 | 0 | 0 | 0 | +30 | 50 |
| 8 | 全恢复药水 | potion | 💎 | 0 | 0 | +100 | +50 | 150 |
| 9 | 史莱姆凝胶 | material | 🟢 | 0 | 0 | 0 | 0 | 10 |
| 10 | 哥布林耳朵 | material | 👂 | 0 | 0 | 0 | 0 | 15 |

---

## 怪物表

| ID | 名称 | HP | ATK | DEF | 等级 | 经验 | 巡逻半径 | 仇恨范围 | 攻击CD |
|----|------|-----|-----|-----|------|------|----------|----------|--------|
| 1 | 史莱姆 | 50 | 8 | 2 | 1 | 20 | 80 | 120 | 2000ms |
| 2 | 哥布林 | 80 | 12 | 4 | 2 | 35 | 100 | 150 | 1800ms |
| 3 | 骷髅战士 | 120 | 18 | 8 | 4 | 60 | 90 | 140 | 1500ms |
| 4 | 暗影法师 | 90 | 25 | 3 | 5 | 80 | 120 | 200 | 2200ms |
| 5 | 岩石巨人 | 300 | 30 | 20 | 8 | 200 | 60 | 100 | 2500ms |

### 怪物掉落

| 怪物 | 必掉 | 概率掉落 |
|------|------|----------|
| 史莱姆 | 凝胶(9) x1 | 生命药水(6) 33% |
| 哥布林 | 耳朵(10) x1 | 法力药水(7) 50% |
| 骷髅战士 | 铁剑(1) x1 | 生命药水(6) 50% |
| 暗影法师 | 钢剑(2) + 全恢复(8) | — |
| 岩石巨人 | 铁甲(4) + 戒指(5) + 全恢复(8) | — |

---

## 技能表

| ID | 名称 | 倍率 | MP | CD | 射程 | 图标 |
|----|------|------|-----|-----|------|------|
| 1 | 普通攻击 | 1.0x | 0 | 800ms | 80 | ⚔ |
| 2 | 重击 | 2.0x | 10 | 2000ms | 80 | 💥 |
| 3 | 火球术 | 3.0x | 20 | 3000ms | 200 | 🔥 |
| 4 | 冰冻 | 1.5x | 15 | 4000ms | 150 | ❄ |
| 5 | 治疗术 | 0.0x | 25 | 5000ms | 0 | 💚 |

---

## 任务表

| ID | 名称 | 目标怪物 | 数量 | 经验 | 物品奖励 |
|----|------|----------|------|------|----------|
| 1 | 清除史莱姆 | 史莱姆(1) | 5 | 100 | 生命药水(6) |
| 2 | 哥布林威胁 | 哥布林(2) | 3 | 200 | 法力药水(7) |
| 3 | 骷髅清剿 | 骷髅战士(3) | 2 | 350 | 铁剑(1) |
| 4 | 暗影威胁 | 暗影法师(4) | 1 | 500 | 钢剑(2) |
| 5 | 巨人杀手 | 岩石巨人(5) | 1 | 1000 | 铁甲(4) |

---

## NPC 表

| ID | 名称 | 类型 | 位置 |
|----|------|------|------|
| 1 | 村长·李四 | quest_giver | (200, 200) |
| 2 | 商人·王五 | merchant | (1400, 200) |
| 3 | 治疗师·赵六 | healer | (800, 600) |
| 4 | 铁匠·孙七 | merchant | (1200, 800) |
| 5 | 公会会长 | quest_giver | (400, 1000) |

---

## 升级经验

```
升级所需经验 = 100 × 等级²
等级 1→2: 100 XP
等级 2→3: 400 XP
等级 3→4: 900 XP
等级 4→5: 1600 XP
...
```

升级奖励: +20 MaxHP, +10 MaxMP, +5 ATK, +2 DEF, 满血满蓝

---

## 错误码

| 错误 | 含义 |
|------|------|
| `cooldown` | 技能冷却中 |
| `not_enough_mp` | 法力不足 |
| `invalid_skill` | 无效技能 ID |
| `cannot_equip` | 该物品不可装备 |
| `not_in_inventory` | 背包中没有该物品 |
| `item_not_found` | 掉落物不存在 |
| `too_far` | 距离太远无法拾取 |
| `quest_already_accepted` | 已接受该任务 |
| `quest_not_accepted` | 未接受该任务 |
| `quest_not_complete` | 任务未完成 |
| `cannot_use` | 该物品不可使用 |
| `out_of_range` | 超出攻击范围 |
| `miss` | 攻击未命中 |
