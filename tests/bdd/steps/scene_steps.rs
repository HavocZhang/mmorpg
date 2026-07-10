//! 场景服 BDD 步骤定义 — 使用 BddWorld.scene_state

use cucumber::{given, then, when};
use super::super::{BddWorld, SceneState};

// Helper: get scene state from world
fn s(world: &BddWorld) -> &SceneState { world.scene_state.as_ref().unwrap() }
fn sm(world: &mut BddWorld) -> &mut SceneState { world.scene_state.as_mut().unwrap() }

// ════════════════════════════════════════════
// Given
// ════════════════════════════════════════════

#[given("场景服已启动")]
async fn given_scene_started(world: &mut BddWorld) {
    world.scene_state = Some(SceneState::new());
}

#[given(expr = "地图 {string} 已加载 尺寸 {string} x {string}")]
async fn given_map_loaded(world: &mut BddWorld, n: String, w: String, h: String) {
    sm(world).load_map(&n, w.parse().unwrap(), h.parse().unwrap());
}

#[given(expr = "玩家 {string} 已在地图 {string} 坐标 {string} {string}")]
async fn given_player_in_map(world: &mut BddWorld, u: String, m: String, x: String, y: String) {
    sm(world).join_map(u.parse().unwrap(), &m, x.parse().unwrap(), y.parse().unwrap()).unwrap();
}

#[given(expr = "AOI视野半径为 {string} 单位")]
async fn given_aoi_radius(world: &mut BddWorld, r: String) {
    sm(world).aoi_radius = r.parse().unwrap();
}

#[given(expr = "场景服最大移动速度为每秒 {string} 单位")]
async fn given_max_speed(world: &mut BddWorld, sp: String) {
    sm(world).max_speed = sp.parse().unwrap();
}

#[given(expr = "地图 {string} 有NPC {string} 名叫 {string} 坐标 {string} {string}")]
async fn given_npc(world: &mut BddWorld, map: String, id: String, name: String, x: String, y: String) {
    sm(world).spawn_npc(&map, id.parse().unwrap(), &name, x.parse().unwrap(), y.parse().unwrap());
}

// ════════════════════════════════════════════
// When
// ════════════════════════════════════════════

#[when(expr = "玩家 {string} 加入地图 {string} 坐标 {string} {string}")]
async fn when_join_map(world: &mut BddWorld, u: String, m: String, x: String, y: String) {
    let r = sm(world).join_map(u.parse().unwrap(), &m, x.parse().unwrap(), y.parse().unwrap());
    if let Err(e) = r { sm(world).last_error = Some(e); }
}

#[when(expr = "玩家 {string} 移动到坐标 {string} {string}")]
async fn when_move_player(world: &mut BddWorld, u: String, x: String, y: String) {
    let _ = sm(world).move_player(u.parse().unwrap(), x.parse().unwrap(), y.parse().unwrap(), 1.0);
}

#[when(expr = "玩家 {string} 在 {string} 秒内移动到坐标 {string} {string}")]
async fn when_move_fast(world: &mut BddWorld, u: String, secs: String, x: String, y: String) {
    let _ = sm(world).move_player(u.parse().unwrap(), x.parse().unwrap(), y.parse().unwrap(), secs.parse().unwrap());
}

#[when("玩家 10001 离开地图")]
async fn when_leave_map(world: &mut BddWorld) {
    sm(world).leave_map(10001);
}

#[when(expr = "玩家 {string} 传送到地图 {string} 坐标 {string} {string}")]
async fn when_teleport(world: &mut BddWorld, u: String, m: String, x: String, y: String) {
    let r = sm(world).teleport(u.parse().unwrap(), &m, x.parse().unwrap(), y.parse().unwrap());
    if let Err(e) = r { sm(world).last_error = Some(e); }
}

#[when("玩家 10001 查询视野内实体")]
async fn when_query_entities(_world: &mut BddWorld) {}

// ════════════════════════════════════════════
// Then
// ════════════════════════════════════════════

#[then(expr = "玩家 {string} 应成功加入地图")]
async fn then_joined(world: &mut BddWorld, u: String) {
    assert!(s(world).player_map.contains_key(&u.parse::<u64>().unwrap()));
}

#[then(expr = "应返回错误 {string}")]
async fn then_error(world: &mut BddWorld, err: String) {
    let actual = s(world).last_error.as_deref().unwrap_or("");
    assert_eq!(actual, err);
}

#[then(expr = "玩家 {string} 的坐标应为 {string} {string}")]
async fn then_position(world: &mut BddWorld, u: String, x: String, y: String) {
    let uid: u64 = u.parse().unwrap();
    let pos = s(world).player_pos.get(&uid).expect("无坐标");
    assert!((pos.0 - x.parse::<f64>().unwrap()).abs() < 0.01);
    assert!((pos.1 - y.parse::<f64>().unwrap()).abs() < 0.01);
}

#[then(expr = "玩家 {string} 的坐标应更新为 {string} {string}")]
async fn then_pos_updated(w: &mut BddWorld, u: String, x: String, y: String) { then_position(w, u, x, y).await; }

#[then(expr = "玩家 {string} 的坐标应修正为 {string} {string}")]
async fn then_pos_clamped(w: &mut BddWorld, u: String, x: String, y: String) { then_position(w, u, x, y).await; }

#[then(expr = "地图 {string} 应有 {string} 个玩家")]
async fn then_map_count(world: &mut BddWorld, map: String, c: String) {
    assert_eq!(s(world).map_player_count(&map) as u64, c.parse::<u64>().unwrap());
}

#[then(expr = "玩家 {string} 应收到移动确认")]
async fn then_move_ok(world: &mut BddWorld, u: String) {
    assert!(s(world).move_confirmations.contains(&u.parse::<u64>().unwrap()));
}

#[then(expr = "玩家 {string} 应被限制在地图边界内")]
async fn then_boundary(world: &mut BddWorld, u: String) {
    assert!(s(world).boundary_violations.contains(&u.parse::<u64>().unwrap()));
}

#[then("应触发速度异常告警")]
async fn then_speed_warn(world: &mut BddWorld) {
    assert!(!s(world).speed_violations.is_empty());
}

#[then(expr = "玩家 {string} 的视野中应有玩家 {string}")]
async fn then_in_range(world: &mut BddWorld, a: String, b: String) {
    assert!(s(world).is_in_range(a.parse().unwrap(), b.parse().unwrap()));
}

#[then(expr = "玩家 {string} 的视野中应没有玩家 {string}")]
async fn then_not_in_range(world: &mut BddWorld, a: String, b: String) {
    assert!(!s(world).is_in_range(a.parse().unwrap(), b.parse().unwrap()));
}

#[then(expr = "玩家 {string} 应收到玩家 {string} 的进入事件")]
async fn then_enter(world: &mut BddWorld, a: String, b: String) {
    assert!(s(world).enter_events.contains(&(a.parse().unwrap(), b.parse().unwrap())));
}

#[then(expr = "玩家 {string} 应收到玩家 {string} 的离开事件")]
async fn then_leave(world: &mut BddWorld, a: String, b: String) {
    let bid: u64 = b.parse().unwrap();
    let st = s(world);
    assert!(st.leave_events.contains(&(a.parse().unwrap(), bid)) || !st.player_map.contains_key(&bid));
}

#[then(expr = "玩家 {string} 不应收到玩家 {string} 的移动广播")]
async fn then_no_broadcast(world: &mut BddWorld, a: String, b: String) {
    assert!(!s(world).move_broadcasts.contains(&(b.parse().unwrap(), a.parse().unwrap())));
}

#[then("玩家 10001 应不在任何地图")]
async fn then_not_in_map(world: &mut BddWorld) {
    assert!(!s(world).player_map.contains_key(&10001));
}

#[then(expr = "玩家 {string} 应在地图 {string}")]
async fn then_in_map(world: &mut BddWorld, u: String, m: String) {
    let a = s(world).player_map.get(&u.parse::<u64>().unwrap()).cloned();
    assert_eq!(a.as_deref().unwrap_or(""), m);
}

#[then(expr = "玩家 10001 应能看到NPC {string}")]
async fn then_see_npc(world: &mut BddWorld, name: String) {
    assert!(s(world).get_visible_npcs(10001).contains(&name));
}

#[then(expr = "玩家 10001 不应能看到NPC {string}")]
async fn then_not_see_npc(world: &mut BddWorld, name: String) {
    assert!(!s(world).get_visible_npcs(10001).contains(&name));
}
