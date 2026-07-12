// ════════════════════════════════════════════════════════════════
// 逻辑服实现 — 背包/装备/物品/商店 (impl GameState)
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use super::state::*;
use super::types::*;
use super::utils::*;
use rust_mmo_gate::grpc_router::proto::gate::*;
use std::sync::atomic::{AtomicU64, Ordering};

// 强化随机源计数器：避免快速循环里 current_millis() 相同导致 roll 固定
static ENHANCE_COUNTER: AtomicU64 = AtomicU64::new(0);

impl GameState {
    // ════════════════════════════════════════════════════════════
    // 拾取物品
    // ════════════════════════════════════════════════════════════
    pub fn handle_pickup(&self, uid: u64, drop_id: u64) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let drop = match self.drops.get(&drop_id) {
            Some(d) => d.clone(),
            None => {
                let err = serde_json::json!({ "error": "item_not_found" }).to_string();
                messages.push(dm(uid, 5003, err, 2));
                return messages;
            }
        };

        // 检查距离
        let player_pos = self.players.get(&uid).map(|p| (p.x, p.y));
        if let Some((px, py)) = player_pos {
            if distance(px, py, drop.x, drop.y) > 60.0 {
                let err = serde_json::json!({ "error": "too_far" }).to_string();
                messages.push(dm(uid, 5003, err, 2));
                return messages;
            }
        }

        self.drops.remove(&drop_id);

        // 添加到背包
        if let Some(mut p) = self.players.get_mut(&uid) {
            p.add_item(drop.item_id, drop.count);
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
        }

        // 广播掉落物被拾取
        let pickup_json = serde_json::json!({
            "dropId": drop_id,
            "pickedBy": uid,
        }).to_string();
        messages.push(dm(0, 6003, pickup_json, 1));

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 装备/卸下
    // ════════════════════════════════════════════════════════════
    pub fn handle_equip(&self, uid: u64, item_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let item = match get_item_def(item_id) {
            Some(i) => i,
            None => return messages,
        };

        if item.item_type == "potion" || item.item_type == "material" {
            let err = serde_json::json!({ "error": "cannot_equip" }).to_string();
            messages.push(dm(uid, 5004, err, 2));
            return messages;
        }

        if let Some(mut p) = self.players.get_mut(&uid) {
            // 检查背包是否有该物品
            if !p.inventory.iter().any(|(id, c)| *id == item_id && *c > 0) {
                let err = serde_json::json!({ "error": "not_in_inventory" }).to_string();
                messages.push(dm(uid, 5004, err, 2));
                return messages;
            }

            let slot = match item.item_type {
                "weapon" => &mut p.weapon,
                "armor" => &mut p.armor,
                "accessory" => &mut p.accessory,
                _ => return messages,
            };

            // 交换装备：旧的放回背包，新的装备上
            let old = *slot;
            *slot = Some(item_id);

            // 从背包移除新装备的物品
            if let Some(entry) = p.inventory.iter_mut().find(|(id, _)| *id == item_id) {
                entry.1 -= 1;
            }

            // 旧装备放回背包
            if let Some(old_id) = old {
                p.add_item(old_id, 1);
            }

            // 发送更新
            messages.push(super::codec::dm_proto(uid, 5004, &p.to_equipment_proto(), 1));
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(super::codec::dm_proto(uid, 5001, &p.to_player_stats(), 1));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 装备强化 (v0.7)
    // slot: "weapon" | "armor" | "accessory"
    // ════════════════════════════════════════════════════════════
    pub fn handle_enhance(&self, uid: u64, slot: &str) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        // 锁内完成判定和修改，提取结果，锁外构建消息
        let result: Option<(u32, u32, bool, String)> = {
            let mut p = match self.players.get_mut(&uid) {
                Some(p) => p,
                None => return messages,
            };

            let (item_id, enhance) = match slot {
                "weapon" => (p.weapon, p.weapon_enhance),
                "armor" => (p.armor, p.armor_enhance),
                "accessory" => (p.accessory, p.accessory_enhance),
                _ => return messages,
            };

            let item_id = match item_id {
                Some(id) => id,
                None => {
                    let err = serde_json::json!({"error":"no_item","slot":slot}).to_string();
                    return vec![dm(uid, 5004, err, 2)];
                }
            };

            // 上限检查
            if enhance >= 10 {
                let err = serde_json::json!({"error":"max_level","slot":slot}).to_string();
                return vec![dm(uid, 5004, err, 2)];
            }

            // 费用 = (当前等级+1) * 100
            let cost = (enhance + 1) * 100;
            if p.gold < cost {
                let err = serde_json::json!({"error":"insufficient_gold","need":cost,"have":p.gold}).to_string();
                return vec![dm(uid, 5004, err, 2)];
            }

            // 成功率: +1~+3=100%, +4~+6=80%, +7~+9=50%, +10=20%
            let success_rate = match enhance + 1 {
                1..=3 => 1.0,
                4..=6 => 0.8,
                7..=9 => 0.5,
                10 => 0.2,
                _ => 1.0,
            };
            // 随机源: uid + 时间 + 自增计数器，避免快速循环里时间相同导致 roll 固定
            let counter = ENHANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let now = current_millis();
            let seed = uid
                .wrapping_mul(2654435761)
                .wrapping_add(now)
                .wrapping_add(counter.wrapping_mul(11400714819323198485));
            let roll = (seed % 10000) as f32 / 10000.0;
            let success = roll < success_rate;

            // 扣金币
            p.gold -= cost;

            let item_def_name = get_item_def(item_id)
                .map(|d| d.name)
                .unwrap_or("装备")
                .to_string();

            if success {
                match slot {
                    "weapon" => p.weapon_enhance += 1,
                    "armor" => p.armor_enhance += 1,
                    "accessory" => p.accessory_enhance += 1,
                    _ => {}
                }
                Some((enhance + 1, p.gold, true, item_def_name))
            } else {
                Some((enhance, p.gold, false, item_def_name))
            }
        };

        // 锁外构建消息
        if let Some((level, gold, success, item_name)) = result {
            let equip_proto = self
                .players
                .get(&uid)
                .map(|p| p.to_equipment_proto())
                .unwrap_or_default();
            messages.push(super::codec::dm_proto(uid, 5004, &equip_proto, 1));

            let stats_proto = self
                .players
                .get(&uid)
                .map(|p| p.to_player_stats())
                .unwrap_or_default();
            messages.push(super::codec::dm_proto(uid, 5001, &stats_proto, 1));

            let msg = if success {
                format!(
                    "强化成功! {} +{} (消耗 {} 金币)",
                    item_name,
                    level,
                    level * 100
                )
            } else {
                format!(
                    "强化失败... {} 仍为 +{} (消耗 {} 金币)",
                    item_name,
                    level,
                    (level + 1) * 100
                )
            };
            let result_json = serde_json::json!({
                "type": "enhance_result",
                "success": success,
                "slot": slot,
                "level": level,
                "gold": gold,
                "message": msg,
            })
            .to_string();
            messages.push(dm(uid, 5006, result_json, 1));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 使用物品
    // ════════════════════════════════════════════════════════════
    pub fn handle_use_item(&self, uid: u64, item_id: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let item = match get_item_def(item_id) {
            Some(i) => i,
            None => return messages,
        };

        if item.item_type != "potion" {
            let err = serde_json::json!({ "error": "cannot_use" }).to_string();
            messages.push(dm(uid, 5003, err, 2));
            return messages;
        }

        if let Some(mut p) = self.players.get_mut(&uid) {
            if !p.remove_item(item_id, 1) {
                let err = serde_json::json!({ "error": "not_in_inventory" }).to_string();
                messages.push(dm(uid, 5003, err, 2));
                return messages;
            }

            if item.hp_restore > 0 {
                p.hp = (p.hp + item.hp_restore).min(p.max_hp);
            }
            if item.mp_restore > 0 {
                p.mp = (p.mp + item.mp_restore).min(p.max_mp);
            }

            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(super::codec::dm_proto(uid, 5001, &p.to_player_stats(), 1));
        }

        messages
    }

    // ════════════════════════════════════════════════════════════
    // 商店系统 (v0.6)
    // ════════════════════════════════════════════════════════════

    /// 从商店购买物品
    pub fn handle_shop_buy(&self, uid: u64, item_id: u32, count: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();
        // 查找商品
        let shop_item = match SHOP_ITEMS.iter().find(|s| s.item_id == item_id) {
            Some(s) => s,
            None => {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":"该物品不在商店中","type":"merchant"}).to_string(), 0));
                return messages;
            }
        };
        // 检查库存
        if let Some(stock) = shop_item.stock {
            if count > stock {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":"库存不足!","type":"merchant"}).to_string(), 0));
                return messages;
            }
        }
        let total_cost = shop_item.price * count;
        // 扣金币 + 加物品
        if let Some(mut p) = self.players.get_mut(&uid) {
            if p.gold < total_cost {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":format!("金币不足! 需要 {} 金", total_cost),"type":"merchant"}).to_string(), 0));
                return messages;
            }
            p.gold -= total_cost;
            // 添加物品到背包
            if let Some(pos) = p.inventory.iter().position(|(id, _)| *id == item_id) {
                p.inventory[pos].1 += count as u32;
            } else {
                p.inventory.push((item_id, count as u32));
            }
            let item_name = get_item_def(item_id).map(|d| d.name).unwrap_or("物品");
            messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":format!("购买 {} x{} 成功! 花费 {} 金币", item_name, count, total_cost),"type":"merchant"}).to_string(), 0));
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(super::codec::dm_proto(uid, 5001, &p.to_player_stats(), 1));
        }
        messages
    }

    /// 出售物品给商店
    pub fn handle_shop_sell(&self, uid: u64, item_id: u32, count: u32) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();
        let shop_item = match SHOP_ITEMS.iter().find(|s| s.item_id == item_id) {
            Some(s) => s,
            None => {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":"商店不收这种物品","type":"merchant"}).to_string(), 0));
                return messages;
            }
        };
        let total_earned = shop_item.sell_price * count;
        if let Some(mut p) = self.players.get_mut(&uid) {
            // 检查是否有足够数量
            if let Some(pos) = p.inventory.iter().position(|(id, c)| *id == item_id && *c >= count as u32) {
                p.inventory[pos].1 -= count as u32;
                if p.inventory[pos].1 == 0 {
                    p.inventory.remove(pos);
                }
            } else {
                messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":"背包中数量不足!","type":"merchant"}).to_string(), 0));
                return messages;
            }
            p.gold += total_earned;
            let item_name = get_item_def(item_id).map(|d| d.name).unwrap_or("物品");
            messages.push(dm(uid, 5006, serde_json::json!({"name":"商店","dialog":format!("出售 {} x{} 成功! 获得 {} 金币", item_name, count, total_earned),"type":"merchant"}).to_string(), 0));
            messages.push(dm(uid, 5003, p.to_inventory_json(), 1));
            messages.push(super::codec::dm_proto(uid, 5001, &p.to_player_stats(), 1));
        }
        messages
    }

    // ════════════════════════════════════════════════════════════
    // 生成掉落物
    // ════════════════════════════════════════════════════════════
    pub fn generate_drops(&self, mob_def_id: u32, x: f32, y: f32) -> Vec<ItemDrop> {
        let mut drops = Vec::new();
        let now = current_millis();

        match mob_def_id {
            1 => { // 史莱姆
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 9, x: x + 10.0, y: y + 10.0, count: 1 });
                if now % 3 == 0 {
                    let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    drops.push(ItemDrop { drop_id, item_id: 6, x: x - 10.0, y: y + 5.0, count: 1 });
                }
            }
            2 => { // 哥布林
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 10, x: x + 10.0, y: y + 10.0, count: 1 });
                if now % 2 == 0 {
                    let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    drops.push(ItemDrop { drop_id, item_id: 7, x: x - 10.0, y: y + 5.0, count: 1 });
                }
            }
            3 => { // 骷髅战士
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 1, x: x + 10.0, y: y + 10.0, count: 1 });
                if now % 2 == 0 {
                    let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    drops.push(ItemDrop { drop_id, item_id: 6, x: x - 10.0, y: y + 5.0, count: 1 });
                }
            }
            4 => { // 暗影法师
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 2, x: x + 10.0, y: y + 10.0, count: 1 });
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x - 10.0, y: y + 5.0, count: 1 });
            }
            5 => { // 岩石巨人
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 4, x: x + 10.0, y: y + 10.0, count: 1 });
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 5, x: x - 10.0, y: y + 5.0, count: 1 });
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x + 5.0, y: y - 10.0, count: 1 });
            }
            // v0.6: Boss 掉落 (全部高价值物品)
            6 => { // 森林守护者
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 2, x: x + 10.0, y: y, count: 1 }); // 钢剑
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 4, x: x - 10.0, y: y, count: 1 }); // 铁甲
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x, y: y + 10.0, count: 3 }); // 全恢复x3
            }
            7 => { // 沙虫领主
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 2, x: x + 10.0, y: y, count: 1 }); // 钢剑
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 5, x: x - 10.0, y: y, count: 2 }); // 力量戒指x2
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x, y: y + 10.0, count: 2 }); // 全恢复x2
            }
            8 => { // 暗黑巫妖王
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 2, x: x + 10.0, y: y, count: 1 }); // 钢剑
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 4, x: x - 10.0, y: y, count: 1 }); // 铁甲
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 5, x: x + 5.0, y: y - 10.0, count: 3 }); // 戒指x3
                let drop_id = self.next_drop_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drops.push(ItemDrop { drop_id, item_id: 8, x: x - 5.0, y: y - 10.0, count: 5 }); // 全恢复x5
            }
            _ => {}
        }

        drops
    }
}
