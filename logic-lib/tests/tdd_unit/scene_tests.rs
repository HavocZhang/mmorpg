//! 场景服 TDD 单元测试 — 测试 src/scene 模块

use logic_lib::scene::aoi::{AoiConfig, AoiEvent, AoiGrid};
use logic_lib::scene::SceneManager;

#[test]
fn test_aoi_grid_create() {
    let grid = AoiGrid::new(&AoiConfig { cell_size: 100.0, map_width: 1000.0, map_height: 1000.0 });
    assert!(grid.is_empty());
    assert_eq!(grid.len(), 0);
}

#[test]
fn test_aoi_add_entity() {
    let mut grid = AoiGrid::new(&AoiConfig { cell_size: 100.0, map_width: 1000.0, map_height: 1000.0 });
    grid.update(1, 150.0, 250.0);
    assert_eq!(grid.len(), 1);
}

#[test]
fn test_aoi_remove_entity() {
    let mut grid = AoiGrid::new(&AoiConfig { cell_size: 100.0, map_width: 1000.0, map_height: 1000.0 });
    grid.update(1, 150.0, 150.0);
    grid.remove(1);
    assert!(grid.is_empty());
}

#[test]
fn test_aoi_nearby_entities_in_range() {
    let mut grid = AoiGrid::new(&AoiConfig { cell_size: 50.0, map_width: 1000.0, map_height: 1000.0 });
    grid.update(1, 100.0, 100.0);
    grid.update(2, 140.0, 140.0); // dist ~56 < 100
    let nearby = grid.query_range(100.0, 100.0, 100.0);
    assert!(nearby.contains(&2));
}

#[test]
fn test_aoi_nearby_entities_out_of_range() {
    let mut grid = AoiGrid::new(&AoiConfig { cell_size: 50.0, map_width: 1000.0, map_height: 1000.0 });
    grid.update(1, 100.0, 100.0);
    grid.update(3, 500.0, 500.0);
    let nearby = grid.query_range(100.0, 100.0, 100.0);
    assert!(!nearby.contains(&3));
}

#[test]
fn test_aoi_enter_events() {
    let mut grid = AoiGrid::new(&AoiConfig { cell_size: 100.0, map_width: 1000.0, map_height: 1000.0 });
    grid.update(1, 50.0, 50.0);
    let events = grid.update(2, 120.0, 120.0);
    let has_enter = events.iter().any(|e| matches!(e, AoiEvent::Enter { entity: 1, observer: 2 }));
    assert!(has_enter);
}

#[test]
fn test_aoi_large_scale() {
    let mut grid = AoiGrid::new(&AoiConfig { cell_size: 50.0, map_width: 10000.0, map_height: 10000.0 });
    for i in 0..1000 {
        grid.update(i, (i * 10) as f64 % 10000.0, (i * 7) as f64 % 10000.0);
    }
    assert_eq!(grid.len(), 1000);
}

#[test]
fn test_scene_manager_join_leave() {
    let mut mgr = SceneManager::new();
    mgr.load_map("map1", 1000.0, 1000.0, 100.0);
    mgr.join(1, "map1", 100.0, 200.0).unwrap();
    assert_eq!(mgr.player_count("map1"), 1);
    mgr.leave(1);
    assert_eq!(mgr.player_count("map1"), 0);
}

#[test]
fn test_scene_manager_move() {
    let mut mgr = SceneManager::new();
    mgr.load_map("map1", 1000.0, 1000.0, 100.0);
    mgr.join(1, "map1", 100.0, 100.0).unwrap();
    mgr.move_player(1, 500.0, 600.0).unwrap();
    assert_eq!(mgr.get_position(1), Some((500.0, 600.0)));
}

#[test]
fn test_scene_manager_teleport() {
    let mut mgr = SceneManager::new();
    mgr.load_map("map1", 1000.0, 1000.0, 100.0);
    mgr.load_map("map2", 500.0, 500.0, 100.0);
    mgr.join(1, "map1", 100.0, 100.0).unwrap();
    mgr.teleport(1, "map2", 200.0, 300.0).unwrap();
    assert_eq!(mgr.get_map(1), Some("map2"));
    assert_eq!(mgr.player_count("map1"), 0);
    assert_eq!(mgr.player_count("map2"), 1);
}

#[test]
fn test_scene_manager_boundary_clamp() {
    let mut mgr = SceneManager::new();
    mgr.load_map("map1", 1000.0, 1000.0, 100.0);
    mgr.join(1, "map1", 100.0, 100.0).unwrap();
    mgr.move_player(1, -100.0, 2000.0).unwrap();
    let pos = mgr.get_position(1).unwrap();
    assert_eq!(pos.0, 0.0);
    assert_eq!(pos.1, 1000.0);
}

#[test]
fn test_scene_manager_map_not_found() {
    let mut mgr = SceneManager::new();
    assert!(mgr.join(1, "no_such_map", 0.0, 0.0).is_err());
}

#[test]
fn test_scene_manager_duplicate_join() {
    let mut mgr = SceneManager::new();
    mgr.load_map("map1", 1000.0, 1000.0, 100.0);
    mgr.join(1, "map1", 100.0, 100.0).unwrap();
    mgr.join(1, "map1", 500.0, 500.0).unwrap();
    // 应更新位置而非创建新条目
    assert_eq!(mgr.player_count("map1"), 1);
    assert_eq!(mgr.get_position(1), Some((500.0, 500.0)));
}
