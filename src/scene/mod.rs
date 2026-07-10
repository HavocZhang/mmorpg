//! 场景管理器 — 地图加载/卸载、玩家进出、移动同步、跨地图传送
//!
//! 每张地图维护独立的 AOI 九宫格，玩家移动时触发视野进出事件。

pub mod aoi;

use std::collections::HashMap;
use aoi::{AoiConfig, AoiEvent, AoiGrid};

/// 场景管理器
pub struct SceneManager {
    maps: HashMap<String, SceneMap>,
    /// player_uid -> map_name
    player_map: HashMap<u64, String>,
}

struct SceneMap {
    name: String,
    width: f64,
    height: f64,
    aoi: AoiGrid,
}

impl SceneManager {
    pub fn new() -> Self {
        Self { maps: HashMap::new(), player_map: HashMap::new() }
    }

    /// 加载地图
    pub fn load_map(&mut self, name: &str, width: f64, height: f64, cell_size: f64) {
        let config = AoiConfig { cell_size, map_width: width, map_height: height };
        self.maps.insert(name.into(), SceneMap {
            name: name.into(),
            width,
            height,
            aoi: AoiGrid::new(&config),
        });
    }

    /// 玩家加入地图
    pub fn join(&mut self, uid: u64, map_name: &str, x: f64, y: f64) -> Result<Vec<AoiEvent>, String> {
        // 检查地图是否存在
        if !self.maps.contains_key(map_name) {
            return Err("地图不存在".into());
        }

        // 如果玩家已在其他地图，先离开
        let need_leave = self.player_map.get(&uid)
            .map(|old| old != map_name)
            .unwrap_or(false);
        if need_leave {
            self.leave(uid);
        }

        // 如果已在此地图，直接更新位置
        if self.player_map.get(&uid) == Some(&map_name.to_string()) {
            let map = self.maps.get_mut(map_name).unwrap();
            let cx = x.max(0.0).min(map.width);
            let cy = y.max(0.0).min(map.height);
            return Ok(map.aoi.update(uid, cx, cy));
        }

        let map = self.maps.get_mut(map_name).unwrap();
        let cx = x.max(0.0).min(map.width);
        let cy = y.max(0.0).min(map.height);
        self.player_map.insert(uid, map_name.into());
        Ok(map.aoi.update(uid, cx, cy))
    }

    /// 玩家离开地图
    pub fn leave(&mut self, uid: u64) -> Vec<AoiEvent> {
        let map_name = match self.player_map.remove(&uid) {
            Some(m) => m,
            None => return vec![],
        };
        match self.maps.get_mut(&map_name) {
            Some(map) => map.aoi.remove(uid),
            None => vec![],
        }
    }

    /// 玩家移动
    pub fn move_player(&mut self, uid: u64, x: f64, y: f64) -> Result<Vec<AoiEvent>, String> {
        let map_name = self.player_map.get(&uid).ok_or("玩家不在地图中")?.clone();
        let map = self.maps.get_mut(&map_name).unwrap();
        let cx = x.max(0.0).min(map.width);
        let cy = y.max(0.0).min(map.height);
        Ok(map.aoi.update(uid, cx, cy))
    }

    /// 跨地图传送
    pub fn teleport(&mut self, uid: u64, map_name: &str, x: f64, y: f64) -> Result<Vec<AoiEvent>, String> {
        let leave_events = self.leave(uid);
        let mut join_events = self.join(uid, map_name, x, y)?;
        let mut all = leave_events;
        all.append(&mut join_events);
        Ok(all)
    }

    /// 查询范围内实体
    pub fn query_range(&self, uid: u64, range: f64) -> Result<Vec<u64>, String> {
        let map_name = self.player_map.get(&uid).ok_or("玩家不在地图中")?;
        let map = self.maps.get(map_name).unwrap();
        let pos = map.aoi.get_position(uid).unwrap_or((0.0, 0.0));
        Ok(map.aoi.query_range(pos.0, pos.1, range))
    }

    /// 获取地图玩家数
    pub fn player_count(&self, map_name: &str) -> usize {
        self.player_map.values().filter(|m| m.as_str() == map_name).count()
    }

    /// 获取玩家位置
    pub fn get_position(&self, uid: u64) -> Option<(f64, f64)> {
        let map_name = self.player_map.get(&uid)?;
        self.maps.get(map_name)?.aoi.get_position(uid)
    }

    /// 获取玩家所在地图
    pub fn get_map(&self, uid: u64) -> Option<&str> {
        self.player_map.get(&uid).map(|s| s.as_str())
    }

    /// 地图总数
    pub fn map_count(&self) -> usize {
        self.maps.len()
    }

    /// 总玩家数
    pub fn total_players(&self) -> usize {
        self.player_map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> SceneManager {
        let mut mgr = SceneManager::new();
        mgr.load_map("test_map", 1000.0, 1000.0, 100.0);
        mgr
    }

    #[test]
    fn test_load_map() {
        let mgr = setup();
        assert_eq!(mgr.map_count(), 1);
    }

    #[test]
    fn test_join_leave() {
        let mut mgr = setup();
        mgr.join(1, "test_map", 100.0, 200.0).unwrap();
        assert_eq!(mgr.player_count("test_map"), 1);
        assert_eq!(mgr.get_map(1), Some("test_map"));

        mgr.leave(1);
        assert_eq!(mgr.player_count("test_map"), 0);
        assert_eq!(mgr.get_map(1), None);
    }

    #[test]
    fn test_join_nonexistent_map() {
        let mut mgr = setup();
        assert!(mgr.join(1, "no_such_map", 0.0, 0.0).is_err());
    }

    #[test]
    fn test_move_player() {
        let mut mgr = setup();
        mgr.join(1, "test_map", 100.0, 100.0).unwrap();
        mgr.move_player(1, 300.0, 400.0).unwrap();
        assert_eq!(mgr.get_position(1), Some((300.0, 400.0)));
    }

    #[test]
    fn test_move_boundary_clamp() {
        let mut mgr = setup();
        mgr.join(1, "test_map", 100.0, 100.0).unwrap();
        mgr.move_player(1, -50.0, 1200.0).unwrap();
        let pos = mgr.get_position(1).unwrap();
        assert_eq!(pos.0, 0.0);
        assert_eq!(pos.1, 1000.0);
    }

    #[test]
    fn test_teleport() {
        let mut mgr = setup();
        mgr.load_map("map2", 500.0, 500.0, 100.0);
        mgr.join(1, "test_map", 100.0, 100.0).unwrap();
        mgr.teleport(1, "map2", 200.0, 300.0).unwrap();
        assert_eq!(mgr.get_map(1), Some("map2"));
        assert_eq!(mgr.player_count("test_map"), 0);
        assert_eq!(mgr.player_count("map2"), 1);
    }

    #[test]
    fn test_query_range() {
        let mut mgr = setup();
        mgr.join(1, "test_map", 100.0, 100.0).unwrap();
        mgr.join(2, "test_map", 150.0, 150.0).unwrap();
        mgr.join(3, "test_map", 500.0, 500.0).unwrap();

        let nearby = mgr.query_range(1, 100.0).unwrap();
        assert!(nearby.contains(&2));
        assert!(!nearby.contains(&3));
    }
}
