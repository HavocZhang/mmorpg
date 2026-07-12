//! Proto 消息编解码
//!
//! 上行消息编码:
//! - LoginRequest (msg_id=1) 在网关层用 JSON 解析，所以登录用 JSON 编码
//! - 其他上行消息 (MoveRequest, AttackRequest 等) 用 proto 编码
//!
//! 下行消息解码:
//! - 5001 PlayerStats, 8004 EntityPosition, 9002 EntityList 等用 proto 解码
//! - 9100 配置数据用 JSON

use prost::Message;
use rust_mmo_gate::game_proto as gp;

// ============================================================================
// 上行消息编码 (msg_id 约定)
// 1001=攻击 1002=技能 1003=拾取 1004=装备 1005=接任务 1006=完成任务
// 1007=NPC交互 1008=用物品 1009=商店买 1010=商店卖 1011=强化
// 2001=聊天 3001=移动 101=拉配置 4002=查询实体
// ============================================================================

/// 编码登录消息 (JSON 格式，网关层用 JSON 解析)
///
/// 网关的 HandshakePayload 期望: {uid, token, version, timestamp}
pub fn encode_login_json(uid: u64, token: &str) -> Vec<u8> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let json = serde_json::json!({
        "uid": uid,
        "token": token,
        "version": 1,
        "timestamp": timestamp,
    });
    serde_json::to_vec(&json).unwrap_or_default()
}

/// 编码移动请求 (3001, proto)
pub fn encode_move(x: f32, y: f32, dir: u32) -> Vec<u8> {
    let msg = gp::MoveRequest { x, y, dir };
    msg.encode_to_vec()
}

/// 编码攻击请求 (1001, proto)
pub fn encode_attack(target_uid: u64) -> Vec<u8> {
    let msg = gp::AttackRequest { target_uid };
    msg.encode_to_vec()
}

/// 编码技能攻击请求 (1002, proto)
pub fn encode_skill(skill_id: u32, target_uid: u64) -> Vec<u8> {
    let msg = gp::SkillAttackRequest {
        skill_id,
        target_uid,
    };
    msg.encode_to_vec()
}

/// 编码拾取请求 (1003, proto)
pub fn encode_pickup(drop_id: u64) -> Vec<u8> {
    let msg = gp::PickupRequest { drop_id };
    msg.encode_to_vec()
}

/// 编码装备请求 (1004, proto)
pub fn encode_equip(item_id: u32, slot: &str) -> Vec<u8> {
    let msg = gp::EquipRequest {
        item_id,
        slot: slot.to_string(),
    };
    msg.encode_to_vec()
}

/// 编码接受任务请求 (1005, proto)
pub fn encode_accept_quest(quest_id: u32) -> Vec<u8> {
    let msg = gp::AcceptQuestRequest { quest_id };
    msg.encode_to_vec()
}

/// 编码完成任务请求 (1006, proto)
pub fn encode_complete_quest(quest_id: u32) -> Vec<u8> {
    let msg = gp::CompleteQuestRequest { quest_id };
    msg.encode_to_vec()
}

/// 编码NPC交互请求 (1007, proto)
pub fn encode_npc_interact(npc_id: u32) -> Vec<u8> {
    let msg = gp::NpcInteractRequest { npc_id };
    msg.encode_to_vec()
}

/// 编码使用物品请求 (1008, proto)
pub fn encode_use_item(item_id: u32) -> Vec<u8> {
    let msg = gp::UseItemRequest { item_id };
    msg.encode_to_vec()
}

/// 编码商店购买请求 (1009, proto)
pub fn encode_shop_buy(item_id: u32, count: u32) -> Vec<u8> {
    let msg = gp::ShopBuyRequest { item_id, count };
    msg.encode_to_vec()
}

/// 编码商店出售请求 (1010, proto)
pub fn encode_shop_sell(item_id: u32, count: u32) -> Vec<u8> {
    let msg = gp::ShopSellRequest { item_id, count };
    msg.encode_to_vec()
}

/// 编码装备强化请求 (1011, proto)
pub fn encode_enhance(slot: &str) -> Vec<u8> {
    let msg = gp::EnhanceRequest {
        slot: slot.to_string(),
    };
    msg.encode_to_vec()
}

/// 编码查询实体请求 (4002, JSON 无字段)
pub fn encode_query_entities() -> Vec<u8> {
    b"{}".to_vec()
}

/// 编码拉取配置请求 (101, JSON 无字段)
pub fn encode_query_config() -> Vec<u8> {
    b"{}".to_vec()
}

// ============================================================================
// 下行消息解码 (proto)
// ============================================================================

/// 解码玩家属性 (5001)
pub fn decode_player_stats(data: &[u8]) -> Option<gp::PlayerStats> {
    gp::PlayerStats::decode(data).ok()
}

/// 解码经验/MP更新 (5002)
pub fn decode_exp_update(data: &[u8]) -> Option<gp::ExpUpdate> {
    gp::ExpUpdate::decode(data).ok()
}

/// 解码背包更新 (5003)
pub fn decode_inventory_update(data: &[u8]) -> Option<gp::InventoryUpdate> {
    gp::InventoryUpdate::decode(data).ok()
}

/// 解码装备更新 (5004)
pub fn decode_equipment_update(data: &[u8]) -> Option<gp::EquipmentUpdate> {
    gp::EquipmentUpdate::decode(data).ok()
}

/// 解码任务更新 (5005)
pub fn decode_quest_update(data: &[u8]) -> Option<gp::QuestUpdate> {
    gp::QuestUpdate::decode(data).ok()
}

/// 解码NPC对话 (5006)
pub fn decode_npc_dialog(data: &[u8]) -> Option<gp::NpcDialog> {
    gp::NpcDialog::decode(data).ok()
}

/// 解码战斗结果 (6001)
pub fn decode_combat_result(data: &[u8]) -> Option<gp::CombatResult> {
    gp::CombatResult::decode(data).ok()
}

/// 解码实体状态 (6002)
pub fn decode_entity_state(data: &[u8]) -> Option<gp::EntityState> {
    gp::EntityState::decode(data).ok()
}

/// 解码实体死亡 (6003)
pub fn decode_entity_death(data: &[u8]) -> Option<gp::EntityDeath> {
    gp::EntityDeath::decode(data).ok()
}

/// 解码玩家位置 (8001)
pub fn decode_player_position(data: &[u8]) -> Option<gp::PlayerPosition> {
    gp::PlayerPosition::decode(data).ok()
}

/// 解码玩家进入 (8002)
pub fn decode_player_enter(data: &[u8]) -> Option<gp::PlayerEnter> {
    gp::PlayerEnter::decode(data).ok()
}

/// 解码玩家离开 (8003)
pub fn decode_player_leave(data: &[u8]) -> Option<gp::PlayerLeave> {
    gp::PlayerLeave::decode(data).ok()
}

/// 解码实体位置 (8004)
pub fn decode_entity_position(data: &[u8]) -> Option<gp::EntityPosition> {
    gp::EntityPosition::decode(data).ok()
}

/// 解码实体列表 (9002)
pub fn decode_entity_list(data: &[u8]) -> Option<gp::EntityList> {
    gp::EntityList::decode(data).ok()
}

// ============================================================================
// JSON 辅助
// ============================================================================

/// 检查 payload 是否为 JSON (以 '{' 开头)
pub fn is_json_payload(data: &[u8]) -> bool {
    !data.is_empty() && data[0] == 0x7B // '{'
}
