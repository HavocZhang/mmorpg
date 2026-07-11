// ════════════════════════════════════════════════════════════════
// 辅助函数
// ════════════════════════════════════════════════════════════════

use super::constants::*;
use rust_mmo_gate::grpc_router::proto::gate::DownstreamMessage;

pub fn get_item_def(id: u32) -> Option<&'static ItemDef> {
    ITEM_DEFS.iter().find(|d| d.id == id)
}

pub fn get_mob_def(id: u32) -> Option<&'static MobDef> {
    MOB_DEFS.iter().find(|d| d.id == id)
}

pub fn get_quest_def(id: u32) -> Option<&'static QuestDef> {
    QUEST_DEFS.iter().find(|d| d.id == id)
}

pub fn get_skill_def(id: u32) -> Option<&'static SkillDef> {
    SKILLS.iter().find(|d| d.id == id)
}

pub fn current_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn distance(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    (dx * dx + dy * dy).sqrt()
}

pub fn dm(target_uid: u64, msg_id: u32, payload: String, priority: u32) -> DownstreamMessage {
    DownstreamMessage {
        target_uid,
        msg_id,
        payload: payload.into_bytes(),
        priority,
    }
}
