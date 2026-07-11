// ════════════════════════════════════════════════════════════════
// Protobuf 协议 SDK — 基于 game.proto 自动生成的 game_proto.json
// 使用方式（浏览器）：
//   1. 先通过 CDN 引入 protobufjs：
//      <script src="https://cdn.jsdelivr.net/npm/protobufjs@7/dist/protobuf.min.js"></script>
//   2. 引入本文件：<script src="sdk/proto_sdk.js"></script>
//   3. 初始化（异步，加载 JSON 描述）：
//      await ProtoSDK.init();
//   4. 同步编码/解码：
//      const buf = ProtoSDK.encode(5001, { uid: 1, name: 'test', hp: 100, ... });
//      const msg = ProtoSDK.decode(5001, buf);
// 使用方式（Node.js）：
//   const { init, encode, decode } = require('./proto_sdk');
//   await init();
//   const buf = encode(5001, { ... });
// ════════════════════════════════════════════════════════════════

// msg_id → 消息类型名映射（对应 game.proto 中的消息定义）
const MSG_TYPE_MAP = {
  // 上行（Client → Server）
  1: 'LoginRequest',
  1001: 'AttackRequest',
  1002: 'SkillAttackRequest',
  1003: 'PickupRequest',
  1004: 'EquipRequest',
  1005: 'AcceptQuestRequest',
  1006: 'CompleteQuestRequest',
  1007: 'NpcInteractRequest',
  1008: 'UseItemRequest',
  1009: 'ShopBuyRequest',
  1010: 'ShopSellRequest',
  1011: 'EnhanceRequest',
  2001: 'ChatRequest',
  2002: 'PartyInviteRequest',
  2003: 'PartyAcceptRequest',
  2004: 'PartyLeaveRequest',
  3001: 'MoveRequest',
  4001: 'QueryPlayersRequest',
  4002: 'QueryEntitiesRequest',
  // 下行（Server → Client）
  5001: 'PlayerStats',
  5002: 'ExpUpdate',
  5003: 'InventoryUpdate',
  5004: 'EquipmentUpdate',
  5005: 'QuestUpdate',
  5006: 'NpcDialog',
  6001: 'CombatResult',
  6002: 'EntityState',
  6003: 'EntityDeath',
  7001: 'ChatAck',
  7002: 'ChatBroadcast',
  8001: 'PlayerPosition',
  8002: 'PlayerEnter',
  8003: 'PlayerLeave',
  8004: 'EntityPosition',
  9001: 'PlayerList',
  9002: 'EntityList',
};

// 反向映射：消息类型名 → msg_id（方便服务端下行消息查找 ID）
const TYPE_MSG_MAP = {};
for (const [id, name] of Object.entries(MSG_TYPE_MAP)) {
  TYPE_MSG_MAP[name] = Number(id);
}

let _root = null;       // protobufjs Root 实例
let _msgTypes = null;   // { msgId: Type }
let _wrapperType = null; // game.GameMessage

// 初始化：加载 JSON 描述并查找所有消息类型
// jsonUrl: game_proto.json 的路径，浏览器默认 'sdk/game_proto.json'，Node 传相对/绝对路径
// 返回 Promise<{ root, msgTypes }>
function init(jsonUrl) {
  if (_root) return Promise.resolve({ root: _root, msgTypes: _msgTypes });
  const url = jsonUrl || 'sdk/game_proto.json';

  // Node.js 用 fs 读取（fetch 不支持相对文件路径），浏览器用 fetch
  const isNode = typeof window === 'undefined'
    && typeof process !== 'undefined'
    && process.versions && process.versions.node;

  let loadJson;
  if (isNode) {
    const fs = require('fs');
    const path = require('path');
    loadJson = Promise.resolve(JSON.parse(fs.readFileSync(path.resolve(url), 'utf8')));
  } else {
    loadJson = fetch(url).then((r) => r.json());
  }

  return loadJson.then((json) => {
    const protobuf = isNode ? require('protobufjs') : window.protobuf;
    _root = protobuf.Root.fromJSON(json);
    _msgTypes = {};
    for (const [msgId, typeName] of Object.entries(MSG_TYPE_MAP)) {
      _msgTypes[msgId] = _root.lookupType(`game.${typeName}`);
    }
    _wrapperType = _root.lookupType('game.GameMessage');
    return { root: _root, msgTypes: _msgTypes };
  });
}

// 获取指定 msgId 的 Type（内部）
function _getType(msgId) {
  if (!_msgTypes) throw new Error('ProtoSDK 未初始化，请先调用 await ProtoSDK.init()');
  const type = _msgTypes[msgId];
  if (!type) throw new Error(`未知消息ID: ${msgId}`);
  return type;
}

// 编码：msgId + data → Uint8Array
function encode(msgId, data) {
  const type = _getType(msgId);
  const errMsg = type.verify(data);
  if (errMsg) throw new Error(`消息 ${type.name} 验证失败: ${errMsg}`);
  return type.encode(data).finish();
}

// 解码：msgId + buffer → plain object
function decode(msgId, buffer) {
  const type = _getType(msgId);
  return type.decode(buffer).toJSON();
}

// 异步编码（自动 init）
function encodeAsync(msgId, data) {
  return init().then(() => encode(msgId, data));
}

// 异步解码（自动 init）
function decodeAsync(msgId, buffer) {
  return init().then(() => decode(msgId, buffer));
}

// 按类型名编码/解码（用于嵌套类型如 InventoryItem 等）
function encodeByName(typeName, data) {
  if (!_root) throw new Error('ProtoSDK 未初始化，请先调用 await ProtoSDK.init()');
  const type = _root.lookupType(`game.${typeName}`);
  const errMsg = type.verify(data);
  if (errMsg) throw new Error(`消息 ${typeName} 验证失败: ${errMsg}`);
  return type.encode(data).finish();
}

function decodeByName(typeName, buffer) {
  if (!_root) throw new Error('ProtoSDK 未初始化，请先调用 await ProtoSDK.init()');
  return _root.lookupType(`game.${typeName}`).decode(buffer).toJSON();
}

// 包装为 GameMessage（统一上行/下行封装）
// direction: 1=UPSTREAM, 2=DOWNSTREAM
// 返回 Uint8Array（可直接发送）
function wrap(msgId, direction, payload, targetUid) {
  if (!_wrapperType) throw new Error('ProtoSDK 未初始化，请先调用 await ProtoSDK.init()');
  const msg = {
    msgId: msgId,
    direction: direction,
    payload: payload,
    targetUid: targetUid || 0,
  };
  const errMsg = _wrapperType.verify(msg);
  if (errMsg) throw new Error(`GameMessage 验证失败: ${errMsg}`);
  return _wrapperType.encode(msg).finish();
}

// 解包 GameMessage → { msgId, direction, payload, targetUid }
function unwrap(buffer) {
  if (!_wrapperType) throw new Error('ProtoSDK 未初始化，请先调用 await ProtoSDK.init()');
  return _wrapperType.decode(buffer).toJSON();
}

// 浏览器全局导出
if (typeof window !== 'undefined') {
  window.ProtoSDK = {
    init, encode, decode, encodeAsync, decodeAsync,
    encodeByName, decodeByName, wrap, unwrap,
    MSG_TYPE_MAP, TYPE_MSG_MAP,
  };
}

// CommonJS 导出（Node.js）
if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    init, encode, decode, encodeAsync, decodeAsync,
    encodeByName, decodeByName, wrap, unwrap,
    MSG_TYPE_MAP, TYPE_MSG_MAP,
  };
}
