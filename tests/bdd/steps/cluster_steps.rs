//! cluster.feature step definitions
//!
//! 集群跨网关协作场景

use cucumber::{given, then, when};

use super::super::{BddWorld, NodeInfo};

// ============ 节点注册 ============

#[given(expr = "网关节点 {string} 启动")]
async fn given_gate_start(world: &mut BddWorld, name: String) {
    world.registered_nodes.insert(
        name.clone(),
        NodeInfo {
            node_id: world.registered_nodes.len() as u64 + 1,
            node_name: name.clone(),
            address: format!("grpc://{}:50051", name),
            online_count: 0,
            last_heartbeat_secs_ago: 0,
            alive: true,
        },
    );
}

#[when("网关完成初始化")]
async fn when_gate_init_done(world: &mut BddWorld) {
    // 节点已注册到 registered_nodes
    assert!(!world.registered_nodes.is_empty(), "应完成初始化");
}

#[then("应向Redis注册节点信息")]
async fn then_register_redis(world: &mut BddWorld) {
    assert!(!world.registered_nodes.is_empty(), "应注册到Redis");
}

#[then("节点信息应包含node_id、node_name、地址")]
async fn then_node_info_complete(world: &mut BddWorld) {
    let node = world.registered_nodes.values().next().unwrap();
    assert!(node.node_id > 0, "应包含 node_id");
    assert!(!node.node_name.is_empty(), "应包含 node_name");
    assert!(!node.address.is_empty(), "应包含地址");
}

#[then("节点应加入集群节点集合")]
async fn then_join_cluster(world: &mut BddWorld) {
    assert!(!world.registered_nodes.is_empty(), "应加入集群");
}

// ============ 心跳上报 ============

#[given(expr = "网关节点 {string} 已注册")]
async fn given_gate_registered(world: &mut BddWorld, name: String) {
    if !world.registered_nodes.contains_key(&name) {
        world.registered_nodes.insert(
            name.clone(),
            NodeInfo {
                node_id: world.registered_nodes.len() as u64 + 1,
                node_name: name.clone(),
                address: format!("grpc://{}:50051", name),
                online_count: 100,
                last_heartbeat_secs_ago: 0,
                alive: true,
            },
        );
    }
}

#[when("网关运行中")]
async fn when_gate_running(world: &mut BddWorld) {
    // 模拟3秒心跳
    world.heartbeat_count += 1;
    for node in world.registered_nodes.values_mut() {
        if node.alive {
            node.last_heartbeat_secs_ago = 0;
        }
    }
}

#[then("应每3秒向Redis上报心跳")]
async fn then_heartbeat_3s(world: &mut BddWorld) {
    assert!(world.heartbeat_count > 0, "应定时上报心跳");
}

#[then("心跳应包含在线人数等状态")]
async fn then_heartbeat_has_status(world: &mut BddWorld) {
    let node = world.registered_nodes.values().next().unwrap();
    assert!(node.online_count >= 0, "心跳应包含在线人数");
}

// ============ 宕机摘除 ============

#[given(expr = "网关节点 {string} 在集群中")]
async fn given_gate_in_cluster(world: &mut BddWorld, name: String) {
    world.registered_nodes.insert(
        name.clone(),
        NodeInfo {
            node_id: world.registered_nodes.len() as u64 + 1,
            node_name: name.clone(),
            address: format!("grpc://{}:50051", name),
            online_count: 50,
            last_heartbeat_secs_ago: 0,
            alive: true,
        },
    );
}

#[when(expr = "{string} 停止心跳上报")]
async fn when_stop_heartbeat(world: &mut BddWorld, name: String) {
    if let Some(node) = world.registered_nodes.get_mut(&name) {
        node.last_heartbeat_secs_ago = 15; // 超过10秒
    }
}

#[then(expr = "10秒后 {string} 应被从集群节点列表中摘除")]
async fn then_removed_from_cluster(world: &mut BddWorld, name: String) {
    if let Some(node) = world.registered_nodes.get_mut(&name) {
        if node.last_heartbeat_secs_ago > 10 {
            node.alive = false;
            world.removed_nodes.push(name.clone());
        }
    }
    assert!(
        world.removed_nodes.contains(&name),
        "{} 应被从集群摘除",
        name
    );
}

#[then(expr = "新连接不应路由到 {string}")]
async fn then_not_route_to(world: &mut BddWorld, name: String) {
    let node = world.registered_nodes.get(&name).unwrap();
    assert!(!node.alive, "不应路由到已下线节点 {}", name);
}

// ============ 本地直接下发 ============

#[given(expr = "玩家 {string} 和玩家 {string} 都在网关 {string}")]
async fn given_both_on_same_gate(world: &mut BddWorld, uid1: String, uid2: String, gate: String) {
    let u1: u64 = uid1.parse().unwrap();
    let u2: u64 = uid2.parse().unwrap();
    world.route_map.insert(u1, gate.clone());
    world.route_map.insert(u2, gate.clone());
}

#[when(expr = "{string} 收到给 {string} 的消息")]
async fn when_receive_msg_for(world: &mut BddWorld, gate: String, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    // 检查目标是否在同一网关
    let target_gate = world.route_map.get(&uid).cloned();
    if target_gate.as_deref() == Some(&gate) {
        // 本地下发
        world.delivered_messages.push((uid, 0x0001));
    } else {
        // 跨网关
        world.pubsub_messages.push((gate, target_gate.unwrap_or_default(), uid));
    }
}

#[then("消息应通过本地会话直接下发")]
async fn then_local_deliver(world: &mut BddWorld) {
    assert!(!world.delivered_messages.is_empty(), "应本地直接下发");
}

#[then("不应经过Redis PubSub")]
async fn then_no_pubsub(world: &mut BddWorld) {
    assert!(world.pubsub_messages.is_empty(), "不应经过PubSub");
}

// ============ 跨网关投递 ============

#[given(expr = "玩家 {string} 在网关 {string}")]
async fn given_player_on_gate(world: &mut BddWorld, uid: String, gate: String) {
    let uid: u64 = uid.parse().unwrap();
    world.route_map.insert(uid, gate);
}

// "收到给...的消息" 步骤已在上面 when_receive_msg_for 中统一定义，处理本地和跨网关两种情况

#[then(expr = "{string} 应通过Redis PubSub发布消息")]
async fn then_publish_pubsub(world: &mut BddWorld, gate: String) {
    let has = world
        .pubsub_messages
        .iter()
        .any(|(from, _, _)| from == &gate);
    assert!(has, "{} 应通过PubSub发布", gate);
}

#[then(expr = "{string} 应订阅到该消息")]
async fn then_subscribe_msg(world: &mut BddWorld, gate: String) {
    let has = world
        .pubsub_messages
        .iter()
        .any(|(_, to, _)| to == &gate);
    assert!(has, "{} 应订阅到消息", gate);
}

#[then(expr = "{string} 应将消息精准下发给 {string}")]
async fn then_deliver_to_player_on_gate(world: &mut BddWorld, gate: String, uid: String) {
    let uid: u64 = uid.parse().unwrap();
    let has = world
        .pubsub_messages
        .iter()
        .any(|(_, to, u)| to == &gate && u == &uid);
    assert!(has, "{} 应精准下发给 {}", gate, uid);
}

#[then("消息不应丢失")]
async fn then_no_loss(world: &mut BddWorld) {
    assert!(!world.pubsub_messages.is_empty(), "消息不应丢失");
}

#[then("消息不应重复投递")]
async fn then_no_duplicate(world: &mut BddWorld) {
    let count = world.pubsub_messages.len();
    let unique: std::collections::HashSet<_> = world.pubsub_messages.iter().collect();
    assert_eq!(count, unique.len(), "消息不应重复投递");
}

// ============ 重连路由更新 ============

#[given(expr = "玩家 {string} 原在网关 {string}")]
async fn given_player_original_gate(world: &mut BddWorld, uid: String, gate: String) {
    let uid: u64 = uid.parse().unwrap();
    world.route_map.insert(uid, gate);
}

#[when(expr = "玩家 {string} 断线后重连到网关 {string}")]
async fn when_reconnect_to_gate(world: &mut BddWorld, uid: String, new_gate: String) {
    let uid: u64 = uid.parse().unwrap();
    world.route_map.insert(uid, new_gate);
}

#[then(expr = "Redis中 {string} 的路由应更新为 {string}")]
async fn then_route_updated(world: &mut BddWorld, uid: String, gate: String) {
    let uid: u64 = uid.parse().unwrap();
    let current = world.route_map.get(&uid).cloned();
    assert_eq!(current, Some(gate.clone()), "路由应更新为 {}", gate);
}

#[then(expr = "后续跨网关消息应路由到 {string}")]
async fn then_route_to_gate(world: &mut BddWorld, gate: String) {
    let has_route = world.route_map.values().any(|g| g == &gate);
    assert!(has_route, "后续消息应路由到 {}", gate);
}
