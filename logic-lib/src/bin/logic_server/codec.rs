// codec.rs — Protobuf 编解码适配层
// logic_server 内部用 proto 类型处理已迁移消息。
// JSON fallback 为兼容旧客户端保留：新客户端默认发 proto，
// 未迁移消息（公会 2501-2503 / 职业 2701 / 天赋 2702 / 排行 2800-2801 /
// 决斗 3100-3101 / 玩家列表 100-9001 / 聊天 ACK 7001 / 聊天广播 7002）仍走 JSON。

use logic_lib::game_proto as gp;
use prost::Message;
use rust_mmo_gate::grpc_router::proto::gate::DownstreamMessage;
use serde_json::Value;

/// 上行消息的统一枚举（handler 用 match 处理）
#[derive(Debug, Clone)]
pub enum UpstreamMsg {
    // LoginRequest 在网关层处理，logic_server 收不到；保留枚举值供未来统一接入
    #[allow(dead_code)]
    LoginRequest(gp::LoginRequest),
    AttackRequest(gp::AttackRequest),
    SkillAttackRequest(gp::SkillAttackRequest),
    PickupRequest(gp::PickupRequest),
    EquipRequest(gp::EquipRequest),
    AcceptQuestRequest(gp::AcceptQuestRequest),
    CompleteQuestRequest(gp::CompleteQuestRequest),
    NpcInteractRequest(gp::NpcInteractRequest),
    UseItemRequest(gp::UseItemRequest),
    ShopBuyRequest(gp::ShopBuyRequest),
    ShopSellRequest(gp::ShopSellRequest),
    EnhanceRequest(gp::EnhanceRequest),
    ChatRequest(gp::ChatRequest),
    PartyInviteRequest(gp::PartyInviteRequest),
    PartyAcceptRequest(gp::PartyAcceptRequest),
    PartyLeaveRequest(gp::PartyLeaveRequest),
    MoveRequest(gp::MoveRequest),
    QueryPlayersRequest(gp::QueryPlayersRequest),
    QueryEntitiesRequest(gp::QueryEntitiesRequest),
    // 未迁移消息（公会/职业/天赋/排行/决斗等）走 JSON fallback
    JsonFallback(Value),
    #[allow(dead_code)]
    Unknown,
}

/// 解码上行消息：先尝试 proto（新客户端默认），失败则 fallback 到 JSON（兼容旧客户端）
pub fn decode_upstream(msg_id: u32, payload: &[u8]) -> UpstreamMsg {
    match msg_id {
        1 => {
            // LoginRequest
            match gp::LoginRequest::decode(payload) {
                Ok(msg) if msg.uid > 0 => UpstreamMsg::LoginRequest(msg),
                _ => {
                    // fallback JSON
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::LoginRequest {
                        uid: json.get("uid").and_then(|v| v.as_u64()).unwrap_or(0),
                        token: json
                            .get("token")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        version: json
                            .get("version")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32,
                        timestamp: json
                            .get("timestamp")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                    };
                    UpstreamMsg::LoginRequest(msg)
                }
            }
        }
        3001 => {
            // MoveRequest
            match gp::MoveRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::MoveRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::MoveRequest {
                        x: json.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                        y: json.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                        dir: json.get("dir").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    };
                    UpstreamMsg::MoveRequest(msg)
                }
            }
        }
        1001 => {
            // AttackRequest
            match gp::AttackRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::AttackRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::AttackRequest {
                        target_uid: json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0),
                    };
                    UpstreamMsg::AttackRequest(msg)
                }
            }
        }
        1002 => {
            // SkillAttackRequest
            match gp::SkillAttackRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::SkillAttackRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::SkillAttackRequest {
                        skill_id: json.get("skillId").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                        target_uid: json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0),
                    };
                    UpstreamMsg::SkillAttackRequest(msg)
                }
            }
        }
        1003 => {
            // PickupRequest
            match gp::PickupRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::PickupRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::PickupRequest {
                        drop_id: json.get("dropId").and_then(|v| v.as_u64()).unwrap_or(0),
                    };
                    UpstreamMsg::PickupRequest(msg)
                }
            }
        }
        1004 => {
            // EquipRequest
            match gp::EquipRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::EquipRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::EquipRequest {
                        item_id: json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                        slot: json
                            .get("slot")
                            .and_then(|v| v.as_str())
                            .unwrap_or("weapon")
                            .to_string(),
                    };
                    UpstreamMsg::EquipRequest(msg)
                }
            }
        }
        1005 => {
            // AcceptQuestRequest
            match gp::AcceptQuestRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::AcceptQuestRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::AcceptQuestRequest {
                        quest_id: json.get("questId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    };
                    UpstreamMsg::AcceptQuestRequest(msg)
                }
            }
        }
        1006 => {
            // CompleteQuestRequest
            match gp::CompleteQuestRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::CompleteQuestRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::CompleteQuestRequest {
                        quest_id: json.get("questId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    };
                    UpstreamMsg::CompleteQuestRequest(msg)
                }
            }
        }
        1007 => {
            // NpcInteractRequest
            match gp::NpcInteractRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::NpcInteractRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::NpcInteractRequest {
                        npc_id: json.get("npcId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    };
                    UpstreamMsg::NpcInteractRequest(msg)
                }
            }
        }
        1008 => {
            // UseItemRequest
            match gp::UseItemRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::UseItemRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::UseItemRequest {
                        item_id: json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    };
                    UpstreamMsg::UseItemRequest(msg)
                }
            }
        }
        1009 => {
            // ShopBuyRequest
            match gp::ShopBuyRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::ShopBuyRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::ShopBuyRequest {
                        item_id: json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                        count: json.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                    };
                    UpstreamMsg::ShopBuyRequest(msg)
                }
            }
        }
        1010 => {
            // ShopSellRequest
            match gp::ShopSellRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::ShopSellRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::ShopSellRequest {
                        item_id: json.get("itemId").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                        count: json.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                    };
                    UpstreamMsg::ShopSellRequest(msg)
                }
            }
        }
        1011 => {
            // EnhanceRequest
            match gp::EnhanceRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::EnhanceRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::EnhanceRequest {
                        slot: json
                            .get("slot")
                            .and_then(|v| v.as_str())
                            .unwrap_or("weapon")
                            .to_string(),
                    };
                    UpstreamMsg::EnhanceRequest(msg)
                }
            }
        }
        2001 => {
            // ChatRequest
            match gp::ChatRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::ChatRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::ChatRequest {
                        text: json
                            .get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        channel: json
                            .get("channel")
                            .and_then(|v| v.as_str())
                            .unwrap_or("world")
                            .to_string(),
                    };
                    UpstreamMsg::ChatRequest(msg)
                }
            }
        }
        2002 => {
            // PartyInviteRequest
            match gp::PartyInviteRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::PartyInviteRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::PartyInviteRequest {
                        target_uid: json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0),
                    };
                    UpstreamMsg::PartyInviteRequest(msg)
                }
            }
        }
        2003 => {
            // PartyAcceptRequest
            match gp::PartyAcceptRequest::decode(payload) {
                Ok(msg) => UpstreamMsg::PartyAcceptRequest(msg),
                _ => {
                    let s = String::from_utf8_lossy(payload);
                    let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
                    let msg = gp::PartyAcceptRequest {
                        inviter_uid: json.get("inviterUid").and_then(|v| v.as_u64()).unwrap_or(0),
                    };
                    UpstreamMsg::PartyAcceptRequest(msg)
                }
            }
        }
        2004 => {
            // PartyLeaveRequest (无字段，直接构造空消息)
            UpstreamMsg::PartyLeaveRequest(gp::PartyLeaveRequest {})
        }
        4001 => {
            // QueryPlayersRequest (无字段)
            UpstreamMsg::QueryPlayersRequest(gp::QueryPlayersRequest {})
        }
        4002 => {
            // QueryEntitiesRequest (无字段)
            UpstreamMsg::QueryEntitiesRequest(gp::QueryEntitiesRequest {})
        }
        _ => {
            // 未迁移消息（公会/职业/天赋/排行/决斗等）走 JSON fallback
            let s = String::from_utf8_lossy(payload);
            let json: Value = serde_json::from_str(&s).unwrap_or(Value::Null);
            UpstreamMsg::JsonFallback(json)
        }
    }
}

/// 下行消息用 proto 编码的辅助函数（8001 PlayerPosition / 8004 EntityPosition 等已迁移下行消息使用）
pub fn dm_proto<T: Message>(
    target_uid: u64,
    msg_id: u32,
    proto_msg: &T,
    priority: u32,
) -> DownstreamMessage {
    DownstreamMessage {
        target_uid,
        msg_id,
        payload: proto_msg.encode_to_vec(),
        priority,
    }
}

/// 构建 PlayerStats 下行消息（proto 编码）
#[allow(dead_code)]
pub fn encode_player_stats(stats: &gp::PlayerStats) -> Vec<u8> {
    stats.encode_to_vec()
}

/// 构建 EntityPosition 下行消息（proto 编码）
#[allow(dead_code)]
pub fn encode_entity_position(pos: &gp::EntityPosition) -> Vec<u8> {
    pos.encode_to_vec()
}
