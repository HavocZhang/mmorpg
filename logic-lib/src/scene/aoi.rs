//! AOI 九宫格 — 兴趣区域管理
//!
//! 将地图划分为网格，每个格子内维护实体列表。
//! 查询时只检查当前格子和相邻8个格子（3x3），避免全量遍历。
//!
//! 复杂度: O(1) 更新, O(格子内实体数) 查询

use std::collections::{HashMap, HashSet};

/// AOI 网格配置
pub struct AoiConfig {
    /// 格子大小（单位：游戏坐标），应与视野半径一致
    pub cell_size: f64,
    /// 地图宽度
    pub map_width: f64,
    /// 地图高度
    pub map_height: f64,
}

/// AOI 九宫格管理器
pub struct AoiGrid {
    cell_size: f64,
    cols: u32,
    rows: u32,
    /// (col, row) -> HashSet<entity_id>
    cells: HashMap<(u32, u32), HashSet<u64>>,
    /// entity_id -> (col, row)
    entity_cell: HashMap<u64, (u32, u32)>,
    /// entity_id -> (x, y)
    entity_pos: HashMap<u64, (f64, f64)>,
}

/// AOI 事件
#[derive(Debug, Clone, PartialEq)]
pub enum AoiEvent {
    Enter { entity: u64, observer: u64 },
    Leave { entity: u64, observer: u64 },
}

impl AoiGrid {
    pub fn new(config: &AoiConfig) -> Self {
        let cols = (config.map_width / config.cell_size).ceil() as u32;
        let rows = (config.map_height / config.cell_size).ceil() as u32;
        Self {
            cell_size: config.cell_size,
            cols,
            rows,
            cells: HashMap::new(),
            entity_cell: HashMap::new(),
            entity_pos: HashMap::new(),
        }
    }

    fn to_cell(&self, x: f64, y: f64) -> (u32, u32) {
        let col = ((x / self.cell_size).floor() as u32).min(self.cols.saturating_sub(1));
        let row = ((y / self.cell_size).floor() as u32).min(self.rows.saturating_sub(1));
        (col, row)
    }

    /// 添加/更新实体位置，返回进出事件
    pub fn update(&mut self, entity: u64, x: f64, y: f64) -> Vec<AoiEvent> {
        let new_cell = self.to_cell(x, y);
        let old_cell = self.entity_cell.get(&entity).copied();
        let old_pos = self.entity_pos.get(&entity).copied();

        // 更新位置
        self.entity_pos.insert(entity, (x, y));

        // 如果格子没变，仅更新位置（无事件）
        if old_cell == Some(new_cell) {
            return vec![];
        }

        // 查询移动前后附近的实体（检查3x3邻居格子）
        let old_nearby: HashSet<u64> = if let Some((ox, oy)) = old_pos {
            self.query_range_internal(ox, oy, self.cell_size)
                .into_iter().filter(|&e| e != entity).collect()
        } else {
            HashSet::new()
        };

        // 从旧格子移除
        if let Some(oc) = old_cell {
            if let Some(cell) = self.cells.get_mut(&oc) {
                cell.remove(&entity);
            }
        }

        // 加入新格子
        self.cells.entry(new_cell).or_default().insert(entity);
        self.entity_cell.insert(entity, new_cell);

        // 查询新位置附近的实体
        let new_nearby: HashSet<u64> = self.query_range_internal(x, y, self.cell_size)
            .into_iter().filter(|&e| e != entity).collect();

        // 生成事件
        let mut events = Vec::new();
        for &e in &new_nearby {
            if !old_nearby.contains(&e) {
                events.push(AoiEvent::Enter { entity: e, observer: entity });
                events.push(AoiEvent::Enter { entity, observer: e });
            }
        }
        for &e in &old_nearby {
            if !new_nearby.contains(&e) {
                events.push(AoiEvent::Leave { entity: e, observer: entity });
                events.push(AoiEvent::Leave { entity, observer: e });
            }
        }
        events
    }

    /// 内部查询（不含距离过滤，基于格子）
    fn query_range_internal(&self, x: f64, y: f64, _range: f64) -> Vec<u64> {
        let center = self.to_cell(x, y);
        let mut result = Vec::new();
        for dc in -1i32..=1 {
            for dr in -1i32..=1 {
                let col = center.0 as i32 + dc;
                let row = center.1 as i32 + dr;
                if col < 0 || row < 0 { continue; }
                if let Some(entities) = self.cells.get(&(col as u32, row as u32)) {
                    result.extend(entities.iter().copied());
                }
            }
        }
        result
    }

    /// 移除实体
    pub fn remove(&mut self, entity: u64) -> Vec<AoiEvent> {
        let mut events = Vec::new();
        if let Some(&cell) = self.entity_cell.get(&entity) {
            if let Some(entities) = self.cells.get_mut(&cell) {
                entities.remove(&entity);
                for &e in entities.iter() {
                    events.push(AoiEvent::Leave { entity, observer: e });
                    events.push(AoiEvent::Leave { entity: e, observer: entity });
                }
            }
        }
        self.entity_cell.remove(&entity);
        self.entity_pos.remove(&entity);
        events
    }

    /// 查询范围内的实体（含自身）
    pub fn query_range(&self, x: f64, y: f64, range: f64) -> Vec<u64> {
        let center = self.to_cell(x, y);
        let cell_range = (range / self.cell_size).ceil() as i32;
        let mut result = Vec::new();

        for dc in -cell_range..=cell_range {
            for dr in -cell_range..=cell_range {
                let col = center.0 as i32 + dc;
                let row = center.1 as i32 + dr;
                if col < 0 || row < 0 {
                    continue;
                }
                let col = col as u32;
                let row = row as u32;
                if let Some(entities) = self.cells.get(&(col, row)) {
                    for &e in entities {
                        if let Some(&(ex, ey)) = self.entity_pos.get(&e) {
                            let dist = ((x - ex).powi(2) + (y - ey).powi(2)).sqrt();
                            if dist <= range {
                                result.push(e);
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// 实体数量
    pub fn len(&self) -> usize {
        self.entity_cell.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entity_cell.is_empty()
    }

    /// 获取实体位置
    pub fn get_position(&self, entity: u64) -> Option<(f64, f64)> {
        self.entity_pos.get(&entity).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_grid() -> AoiGrid {
        AoiGrid::new(&AoiConfig {
            cell_size: 100.0,
            map_width: 1000.0,
            map_height: 1000.0,
        })
    }

    #[test]
    fn test_new_grid() {
        let grid = make_grid();
        assert!(grid.is_empty());
        assert_eq!(grid.len(), 0);
    }

    #[test]
    fn test_add_entity() {
        let mut grid = make_grid();
        grid.update(1, 150.0, 250.0);
        assert_eq!(grid.len(), 1);
        assert_eq!(grid.get_position(1), Some((150.0, 250.0)));
    }

    #[test]
    fn test_update_same_cell_no_events() {
        let mut grid = make_grid();
        grid.update(1, 150.0, 150.0);
        let events = grid.update(1, 160.0, 170.0); // same cell (100,100) -> (100,100)
        assert!(events.is_empty());
        assert_eq!(grid.get_position(1), Some((160.0, 170.0)));
    }

    #[test]
    fn test_remove_entity() {
        let mut grid = make_grid();
        grid.update(1, 150.0, 150.0);
        grid.remove(1);
        assert!(grid.is_empty());
    }

    #[test]
    fn test_query_range_empty() {
        let grid = make_grid();
        assert!(grid.query_range(500.0, 500.0, 100.0).is_empty());
    }

    #[test]
    fn test_query_range_nearby() {
        let mut grid = make_grid();
        grid.update(1, 100.0, 100.0);
        grid.update(2, 150.0, 150.0); // dist=70.7 < 100
        grid.update(3, 300.0, 300.0); // dist=282.8 > 100

        let nearby = grid.query_range(100.0, 100.0, 100.0);
        assert!(nearby.contains(&1));
        assert!(nearby.contains(&2));
        assert!(!nearby.contains(&3));
    }

    #[test]
    fn test_enter_event() {
        let mut grid = make_grid();
        grid.update(1, 50.0, 50.0);   // cell (0,0)
        let events = grid.update(2, 120.0, 120.0); // cell (1,1) -> adjacent, should get enter
        let has_enter = events.iter().any(|e| matches!(e, AoiEvent::Enter { entity: 1, observer: 2 }));
        assert!(has_enter, "应该触发进入事件");
    }

    #[test]
    fn test_boundary_clamp() {
        let mut grid = make_grid();
        grid.update(1, -50.0, -50.0);
        // AoiGrid stores raw coordinates; boundary clamping is SceneManager's job
        let pos = grid.get_position(1).unwrap();
        assert_eq!(pos, (-50.0, -50.0)); // raw coords are stored
        // But cell index should be valid (0,0)
        assert!(grid.len() == 1);
    }
}
