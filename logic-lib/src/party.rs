// 组队系统 — 邀请、加入、离开、解散、经验共享
use std::collections::HashMap;
use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct PartyMember {
    pub uid: u64,
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub level: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct Party {
    pub id: u64,
    pub leader_uid: u64,
    pub members: Vec<PartyMember>,
    pub pending_invites: Vec<u64>,
}

pub struct PartyManager {
    parties: DashMap<u64, Party>,
    uid_to_party: DashMap<u64, u64>,
    next_id: std::sync::atomic::AtomicU64,
}

impl PartyManager {
    pub fn new() -> Self {
        Self {
            parties: DashMap::new(),
            uid_to_party: DashMap::new(),
            next_id: std::sync::atomic::AtomicU64::new(1000),
        }
    }

    /// 创建队伍并邀请目标
    pub fn create_and_invite(&self, leader_uid: u64, leader_name: &str, target_uid: u64) -> u64 {
        if self.uid_to_party.contains_key(&leader_uid) {
            return 0;
        }
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.parties.insert(id, Party {
            id, leader_uid,
            members: vec![PartyMember {
                uid: leader_uid, name: leader_name.to_string(),
                hp: 100, max_hp: 100, level: 1, x: 0.0, y: 0.0,
            }],
            pending_invites: vec![target_uid],
        });
        self.uid_to_party.insert(leader_uid, id);
        id
    }

    /// 接受邀请加入队伍
    pub fn accept_invite(&self, uid: u64, name: &str) -> Option<u64> {
        for mut party in self.parties.iter_mut() {
            if party.pending_invites.contains(&uid) {
                party.pending_invites.retain(|u| *u != uid);
                party.members.push(PartyMember {
                    uid, name: name.to_string(), hp: 100, max_hp: 100, level: 1, x: 0.0, y: 0.0,
                });
                self.uid_to_party.insert(uid, party.id);
                return Some(party.id);
            }
        }
        None
    }

    /// 离开队伍
    pub fn leave(&self, uid: u64) {
        if let Some((_, pid)) = self.uid_to_party.remove(&uid) {
            if let Some(mut party) = self.parties.get_mut(&pid) {
                party.members.retain(|m| m.uid != uid);
                if party.members.is_empty() {
                    drop(party);
                    self.parties.remove(&pid);
                }
            }
        }
    }

    /// 获取队伍 ID
    pub fn get_party_id(&self, uid: u64) -> Option<u64> {
        self.uid_to_party.get(&uid).map(|v| *v.value())
    }

    /// 获取队伍所有成员 UID
    pub fn get_party_members(&self, party_id: u64) -> Vec<u64> {
        self.parties.get(&party_id)
            .map(|p| p.members.iter().map(|m| m.uid).collect())
            .unwrap_or_default()
    }

    /// 计算经验共享（成员均分）
    pub fn share_exp(&self, party_id: u64, total_exp: u32, exclude_uid: u64) -> Vec<(u64, u32)> {
        let party = match self.parties.get(&party_id) {
            Some(p) => p,
            None => return vec![],
        };
        let eligible: Vec<u64> = party.members.iter()
            .filter(|m| m.uid != exclude_uid)
            .map(|m| m.uid).collect();
        if eligible.is_empty() { return vec![]; }
        let share = (total_exp as f32 / eligible.len() as f32).max(1.0) as u32;
        eligible.into_iter().map(|uid| (uid, share)).collect()
    }

    /// 更新队伍成员状态
    pub fn update_member(&self, uid: u64, hp: i32, max_hp: i32, level: u32, x: f32, y: f32, name: &str) {
        if let Some(pid) = self.uid_to_party.get(&uid) {
            if let Some(mut party) = self.parties.get_mut(&pid.value()) {
                for m in &mut party.members {
                    if m.uid == uid {
                        m.hp = hp; m.max_hp = max_hp; m.level = level;
                        m.x = x; m.y = y; m.name = name.to_string();
                    }
                }
            }
        }
    }
}
