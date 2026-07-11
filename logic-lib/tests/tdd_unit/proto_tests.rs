//! 协议层 TDD 测试 — 验证 protobuf 消息定义正确
//!
//! 验证 game.proto 中定义的消息能够被 prost 正确编解码。
//! 这是"客户端无关框架"协议层的基础测试，确保所有消息结构
//! 可以稳定序列化/反序列化，为后续多语言 SDK 生成奠定基础。

use logic_lib::game_proto::*;
use prost::Message;

#[test]
fn test_player_stats_roundtrip() {
    let original = PlayerStats {
        uid: 12345, name: "测试玩家".to_string(),
        hp: 100, max_hp: 100, mp: 50, max_mp: 50,
        level: 5, exp: 200, max_exp: 500,
        x: 400.0, y: 300.0, atk: 20, def: 10,
        gold: 1000, class_id: 1, talent_points: 3,
    };
    let buf = original.encode_to_vec();
    let decoded = PlayerStats::decode(&buf[..]).unwrap();
    assert_eq!(decoded.uid, 12345);
    assert_eq!(decoded.name, "测试玩家");
    assert_eq!(decoded.hp, 100);
    assert_eq!(decoded.x, 400.0);
}

#[test]
fn test_attack_request_roundtrip() {
    let req = AttackRequest { target_uid: 10001 };
    let buf = req.encode_to_vec();
    let decoded = AttackRequest::decode(&buf[..]).unwrap();
    assert_eq!(decoded.target_uid, 10001);
}

#[test]
fn test_move_request_roundtrip() {
    let req = MoveRequest { x: 500.0, y: 300.0, dir: 2 };
    let buf = req.encode_to_vec();
    let decoded = MoveRequest::decode(&buf[..]).unwrap();
    assert_eq!(decoded.x, 500.0);
    assert_eq!(decoded.dir, 2);
}

#[test]
fn test_entity_list_with_multiple_entries() {
    let list = EntityList {
        npcs: vec![
            EntityListEntry { entity_id: 1, def_id: 1, name: "村长".to_string(), x: 200.0, y: 200.0, hp: 100, max_hp: 100, level: 1, npc_type: "quest_giver".to_string(), quest_id: 1 },
        ],
        mobs: vec![
            EntityListEntry { entity_id: 10000, def_id: 1, name: "史莱姆".to_string(), x: 400.0, y: 400.0, hp: 50, max_hp: 50, level: 1, npc_type: String::new(), quest_id: 0 },
            EntityListEntry { entity_id: 10001, def_id: 2, name: "哥布林".to_string(), x: 500.0, y: 400.0, hp: 80, max_hp: 80, level: 2, npc_type: String::new(), quest_id: 0 },
        ],
    };
    let buf = list.encode_to_vec();
    let decoded = EntityList::decode(&buf[..]).unwrap();
    assert_eq!(decoded.npcs.len(), 1);
    assert_eq!(decoded.mobs.len(), 2);
    assert_eq!(decoded.mobs[1].name, "哥布林");
}

#[test]
fn test_game_message_wrapper() {
    let stats = PlayerStats { uid: 1, name: "t".to_string(), hp: 100, max_hp: 100, mp: 50, max_mp: 50, level: 1, exp: 0, max_exp: 100, x: 0.0, y: 0.0, atk: 10, def: 5, gold: 0, class_id: 0, talent_points: 0 };
    let wrapper = GameMessage {
        msg_id: 5001,
        direction: 2, // DOWNSTREAM
        payload: stats.encode_to_vec(),
        target_uid: 1,
    };
    let buf = wrapper.encode_to_vec();
    let decoded = GameMessage::decode(&buf[..]).unwrap();
    assert_eq!(decoded.msg_id, 5001);
    let stats2 = PlayerStats::decode(&decoded.payload[..]).unwrap();
    assert_eq!(stats2.hp, 100);
}
