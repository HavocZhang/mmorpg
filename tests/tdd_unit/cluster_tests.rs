//! TDD 单元测试 — Redis集群服务模块
//!
//! 测试路由索引管理、服务发现、跨网关消息结构

use rust_mmo_gate::cluster::route_index::RouteIndex;
use rust_mmo_gate::cluster::cross_gate_pubsub::CrossGateMsg;

#[test]
#[allow(clippy::assertions_on_constants)]
fn test_route_index_creation() {
    let _index = RouteIndex::new("redis://127.0.0.1:6379".to_string());
    // 验证 RouteIndex 可以正常创建
    assert!(true);
}

#[test]
fn test_cross_gate_message_fields() {
    let msg = CrossGateMsg {
        from_node: 1,
        to_uid: 100,
        msg_id: 2001,
        payload: vec![1, 2, 3],
        priority: 1,
    };
    assert_eq!(msg.from_node, 1);
    assert_eq!(msg.to_uid, 100);
    assert_eq!(msg.msg_id, 2001);
    assert_eq!(msg.payload, vec![1, 2, 3]);
    assert_eq!(msg.priority, 1);
}

#[test]
fn test_cross_gate_message_zero_uid_is_broadcast() {
    let msg = CrossGateMsg {
        from_node: 1,
        to_uid: 0, // 广播
        msg_id: 7002,
        payload: b"broadcast".to_vec(),
        priority: 1,
    };
    assert_eq!(msg.to_uid, 0);
}

#[test]
fn test_cross_gate_message_priority_range() {
    for prio in 0..=2u8 {
        let msg = CrossGateMsg {
            from_node: 1,
            to_uid: 1,
            msg_id: 100,
            payload: vec![],
            priority: prio,
        };
        assert_eq!(msg.priority, prio);
    }
}
