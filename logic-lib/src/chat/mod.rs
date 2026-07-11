//! 聊天管理器 — 频道管理、消息收发、频率限制、敏感词过滤、离线消息
//!
//! 每张频道维护消息历史，频道类型支持世界、私聊、公会、队伍。

use std::collections::{HashMap, HashSet, VecDeque};

/// 频道类型
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelType {
    World,
    Private,
    Guild(String),
    Party(u64),
}

/// 频道信息
#[derive(Debug, Clone)]
pub struct Channel {
    pub name: String,
    pub channel_type: ChannelType,
    pub messages: VecDeque<(u64, String)>,
    pub members: HashSet<u64>,
    pub max_history: usize,
}

impl Channel {
    pub fn new(name: &str, channel_type: ChannelType, max_history: usize) -> Self {
        Self {
            name: name.to_string(),
            channel_type,
            messages: VecDeque::new(),
            members: HashSet::new(),
            max_history,
        }
    }

    pub fn push_message(&mut self, uid: u64, text: &str) {
        if self.messages.len() >= self.max_history {
            self.messages.pop_front();
        }
        self.messages.push_back((uid, text.to_string()));
    }

    pub fn get_history(&self, limit: usize) -> Vec<(u64, String)> {
        let count = limit.min(self.messages.len());
        self.messages.iter().rev().take(count).rev().cloned().collect()
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn add_member(&mut self, uid: u64) {
        self.members.insert(uid);
    }

    pub fn remove_member(&mut self, uid: u64) {
        self.members.remove(&uid);
    }

    pub fn has_member(&self, uid: u64) -> bool {
        self.members.contains(&uid)
    }
}

/// 聊天管理器
pub struct ChatManager {
    channels: HashMap<String, Channel>,
    online_players: HashMap<u64, HashSet<String>>,
    rate_timestamps: HashMap<u64, VecDeque<u64>>,
    max_rate: u32,
    rate_window_secs: u64,
    max_message_length: usize,
    sensitive_words: Vec<String>,
    offline_messages: HashMap<u64, Vec<(u64, String)>>,
    player_guilds: HashMap<u64, String>,
    player_parties: HashMap<u64, u64>,
    pub acks: Vec<u64>,
    pub filtered_count: u32,
    pub rate_limited_count: u32,
    private_messages: Vec<(u64, u64, String)>,
}

impl ChatManager {
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
            online_players: HashMap::new(),
            rate_timestamps: HashMap::new(),
            max_rate: 5,
            rate_window_secs: 1,
            max_message_length: 500,
            sensitive_words: vec!["广告".to_string(), "外挂".to_string(), "私服".to_string()],
            offline_messages: HashMap::new(),
            player_guilds: HashMap::new(),
            player_parties: HashMap::new(),
            acks: Vec::new(),
            filtered_count: 0,
            rate_limited_count: 0,
            private_messages: Vec::new(),
        }
    }

    /// 玩家上线
    pub fn player_online(&mut self, uid: u64) {
        self.online_players.entry(uid).or_default();
    }

    /// 玩家离线
    pub fn player_offline(&mut self, uid: u64) {
        self.online_players.remove(&uid);
    }

    /// 创建频道
    pub fn create_channel(&mut self, name: &str, channel_type: ChannelType) {
        self.channels.entry(name.to_string()).or_insert_with(|| {
            Channel::new(name, channel_type, 100)
        });
    }

    /// 加入频道
    pub fn join_channel(&mut self, uid: u64, channel_name: &str) -> Result<(), String> {
        if !self.online_players.contains_key(&uid) {
            return Err("玩家未上线".to_string());
        }
        let ch = self.channels.entry(channel_name.to_string()).or_insert_with(|| {
            Channel::new(channel_name, ChannelType::World, 100)
        });
        ch.add_member(uid);
        self.online_players.entry(uid).or_default().insert(channel_name.to_string());
        Ok(())
    }

    /// 离开频道
    pub fn leave_channel(&mut self, uid: u64, channel_name: &str) {
        if let Some(ch) = self.channels.get_mut(channel_name) {
            ch.remove_member(uid);
        }
        if let Some(channels) = self.online_players.get_mut(&uid) {
            channels.remove(channel_name);
        }
    }

    /// 加入公会
    pub fn join_guild(&mut self, uid: u64, guild_name: &str) {
        self.player_guilds.insert(uid, guild_name.to_string());
    }

    /// 加入队伍
    pub fn join_party(&mut self, uid: u64, party_id: u64) {
        self.player_parties.insert(uid, party_id);
    }

    /// 频率检查
    fn check_rate(&mut self, uid: u64) -> bool {
        let now = current_timestamp();
        let timestamps = self.rate_timestamps.entry(uid).or_default();
        // 清理过期时间戳
        while let Some(&ts) = timestamps.front() {
            if now - ts > self.rate_window_secs {
                timestamps.pop_front();
            } else {
                break;
            }
        }
        if timestamps.len() >= self.max_rate as usize {
            self.rate_limited_count += 1;
            return false;
        }
        timestamps.push_back(now);
        true
    }

    /// 敏感词过滤
    fn filter_content(&self, text: &str) -> bool {
        for word in &self.sensitive_words {
            if text.contains(word.as_str()) {
                return true;
            }
        }
        false
    }

    /// 发送消息到频道
    pub fn send_to_channel(&mut self, uid: u64, channel_name: &str, text: &str) -> Result<Vec<u64>, String> {
        if text.len() > self.max_message_length {
            return Err("消息过长".to_string());
        }
        if self.filter_content(text) {
            self.filtered_count += 1;
            return Err("消息被过滤".to_string());
        }
        if !self.check_rate(uid) {
            return Err("发送频率过快".to_string());
        }

        let ch = self.channels.entry(channel_name.to_string()).or_insert_with(|| {
            Channel::new(channel_name, ChannelType::World, 100)
        });
        ch.push_message(uid, text);
        self.acks.push(uid);

        // 返回接收者列表
        let receivers: Vec<u64> = ch.members.iter().cloned().collect();
        Ok(receivers)
    }

    /// 发送私聊消息
    pub fn send_private(&mut self, from: u64, to: u64, text: &str) -> Result<(), String> {
        if text.len() > self.max_message_length {
            return Err("消息过长".to_string());
        }
        if self.filter_content(text) {
            self.filtered_count += 1;
            return Err("消息被过滤".to_string());
        }
        if !self.check_rate(from) {
            return Err("发送频率过快".to_string());
        }

        self.private_messages.push((from, to, text.to_string()));
        self.acks.push(from);

        // 如果目标不在线，存储离线消息
        if !self.online_players.contains_key(&to) {
            self.offline_messages.entry(to).or_default().push((from, text.to_string()));
        }
        Ok(())
    }

    /// 发送公会消息
    pub fn send_guild(&mut self, uid: u64, guild_name: &str, text: &str) -> Result<Vec<u64>, String> {
        if text.len() > self.max_message_length {
            return Err("消息过长".to_string());
        }
        if self.filter_content(text) {
            self.filtered_count += 1;
            return Err("消息被过滤".to_string());
        }
        if !self.check_rate(uid) {
            return Err("发送频率过快".to_string());
        }

        self.acks.push(uid);

        // 获取公会成员
        let guild_members: Vec<u64> = self.player_guilds.iter()
            .filter(|(_, g)| g.as_str() == guild_name)
            .map(|(&u, _)| u)
            .collect();
        Ok(guild_members)
    }

    /// 发送队伍消息
    pub fn send_party(&mut self, uid: u64, party_id: u64, text: &str) -> Result<Vec<u64>, String> {
        if text.len() > self.max_message_length {
            return Err("消息过长".to_string());
        }
        if self.filter_content(text) {
            self.filtered_count += 1;
            return Err("消息被过滤".to_string());
        }
        if !self.check_rate(uid) {
            return Err("发送频率过快".to_string());
        }

        self.acks.push(uid);

        let party_members: Vec<u64> = self.player_parties.iter()
            .filter(|(_, &p)| p == party_id)
            .map(|(&u, _)| u)
            .collect();
        Ok(party_members)
    }

    /// 查询历史消息
    pub fn query_history(&self, channel_name: &str, limit: usize) -> Vec<(u64, String)> {
        match self.channels.get(channel_name) {
            Some(ch) => ch.get_history(limit),
            None => Vec::new(),
        }
    }

    /// 获取频道消息数
    pub fn channel_message_count(&self, channel_name: &str) -> usize {
        self.channels.get(channel_name).map(|ch| ch.message_count()).unwrap_or(0)
    }

    /// 获取离线消息
    pub fn get_offline_messages(&self, uid: u64) -> Vec<(u64, String)> {
        self.offline_messages.get(&uid).cloned().unwrap_or_default()
    }

    /// 离线消息数量
    pub fn offline_message_count(&self, uid: u64) -> usize {
        self.offline_messages.get(&uid).map(|v| v.len()).unwrap_or(0)
    }

    /// 获取频道成员数
    pub fn channel_member_count(&self, channel_name: &str) -> usize {
        self.channels.get(channel_name).map(|ch| ch.members.len()).unwrap_or(0)
    }

    /// 设置最大频率
    pub fn set_max_rate(&mut self, rate: u32) {
        self.max_rate = rate;
    }

    /// 设置消息最大长度
    pub fn set_max_message_length(&mut self, len: usize) {
        self.max_message_length = len;
    }

    /// 添加敏感词
    pub fn add_sensitive_word(&mut self, word: &str) {
        self.sensitive_words.push(word.to_string());
    }

    /// 获取私聊消息
    pub fn get_private_messages(&self) -> &Vec<(u64, u64, String)> {
        &self.private_messages
    }

    /// 创建必要的频道（测试辅助）
    pub fn ensure_world_channel(&mut self) {
        self.create_channel("world", ChannelType::World);
    }
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_create() {
        let mut mgr = ChatManager::new();
        mgr.create_channel("world", ChannelType::World);
        assert_eq!(mgr.channel_message_count("world"), 0);
    }

    #[test]
    fn test_message_send_to_channel() {
        let mut mgr = ChatManager::new();
        mgr.player_online(10001);
        mgr.ensure_world_channel();
        mgr.join_channel(10001, "world").unwrap();
        let result = mgr.send_to_channel(10001, "world", "hello");
        assert!(result.is_ok());
        assert_eq!(mgr.channel_message_count("world"), 1);
        assert_eq!(mgr.acks.len(), 1);
    }

    #[test]
    fn test_private_message_targeting() {
        let mut mgr = ChatManager::new();
        mgr.player_online(10001);
        mgr.send_private(10001, 10002, "hi").unwrap();
        assert_eq!(mgr.acks.len(), 1);
        assert_eq!(mgr.private_messages.len(), 1);
        assert_eq!(mgr.private_messages[0], (10001, 10002, "hi".to_string()));
    }

    #[test]
    fn test_message_history_retrieval() {
        let mut mgr = ChatManager::new();
        mgr.player_online(10001);
        mgr.ensure_world_channel();
        mgr.join_channel(10001, "world").unwrap();
        mgr.send_to_channel(10001, "world", "msg1").unwrap();
        mgr.send_to_channel(10001, "world", "msg2").unwrap();
        mgr.send_to_channel(10001, "world", "msg3").unwrap();
        let history = mgr.query_history("world", 10);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].1, "msg1");
        assert_eq!(history[2].1, "msg3");
    }

    #[test]
    fn test_rate_limiting() {
        let mut mgr = ChatManager::new();
        mgr.set_max_rate(2);
        mgr.player_online(10001);
        mgr.ensure_world_channel();
        mgr.join_channel(10001, "world").unwrap();
        assert!(mgr.send_to_channel(10001, "world", "m1").is_ok());
        assert!(mgr.send_to_channel(10001, "world", "m2").is_ok());
        assert!(mgr.send_to_channel(10001, "world", "m3").is_err());
        assert_eq!(mgr.rate_limited_count, 1);
    }

    #[test]
    fn test_message_length_validation() {
        let mut mgr = ChatManager::new();
        mgr.set_max_message_length(200);
        mgr.player_online(10001);
        mgr.ensure_world_channel();
        mgr.join_channel(10001, "world").unwrap();
        let long_text = "a".repeat(250);
        let result = mgr.send_to_channel(10001, "world", &long_text);
        assert!(result.is_err());
    }

    #[test]
    fn test_keyword_filtering() {
        let mut mgr = ChatManager::new();
        mgr.add_sensitive_word("外挂");
        mgr.player_online(10001);
        mgr.ensure_world_channel();
        mgr.join_channel(10001, "world").unwrap();
        let result = mgr.send_to_channel(10001, "world", "加我微信买外挂");
        assert!(result.is_err());
        assert_eq!(mgr.filtered_count, 1);
        assert_eq!(mgr.channel_message_count("world"), 0);
    }

    #[test]
    fn test_offline_message_queue() {
        let mut mgr = ChatManager::new();
        mgr.player_online(10001);
        // Player 10002 is NOT online
        mgr.send_private(10001, 10002, "hello offline").unwrap();
        assert_eq!(mgr.offline_message_count(10002), 1);
        let msgs = mgr.get_offline_messages(10002);
        assert_eq!(msgs[0].1, "hello offline");
    }

    #[test]
    fn test_multi_channel_subscription() {
        let mut mgr = ChatManager::new();
        mgr.player_online(10001);
        mgr.ensure_world_channel();
        mgr.create_channel("guild_1", ChannelType::Guild("guild_1".to_string()));
        mgr.create_channel("party_99", ChannelType::Party(99));
        mgr.join_channel(10001, "world").unwrap();
        mgr.join_channel(10001, "guild_1").unwrap();
        mgr.join_channel(10001, "party_99").unwrap();
        assert_eq!(mgr.channel_member_count("world"), 1);
        assert_eq!(mgr.channel_member_count("guild_1"), 1);
        assert_eq!(mgr.channel_member_count("party_99"), 1);
    }

    #[test]
    fn test_broadcast_to_channel_members() {
        let mut mgr = ChatManager::new();
        mgr.player_online(10001);
        mgr.player_online(10002);
        mgr.player_online(10003);
        mgr.ensure_world_channel();
        mgr.join_channel(10001, "world").unwrap();
        mgr.join_channel(10002, "world").unwrap();
        mgr.join_channel(10003, "world").unwrap();
        let receivers = mgr.send_to_channel(10001, "world", "broadcast").unwrap();
        // All 3 members should be listed, including the sender
        assert_eq!(receivers.len(), 3);
        assert!(receivers.contains(&10002));
        assert!(receivers.contains(&10003));
    }

    #[test]
    fn test_guild_messages_only_to_guild() {
        let mut mgr = ChatManager::new();
        mgr.player_online(10001);
        mgr.player_online(10002);
        mgr.join_guild(10001, "屠龙公会");
        mgr.join_guild(10002, "屠龙公会");
        let receivers = mgr.send_guild(10001, "屠龙公会", "公会消息").unwrap();
        assert!(receivers.contains(&10002));
        assert!(!receivers.contains(&99999));
        assert_eq!(mgr.acks.len(), 1);
    }

    #[test]
    fn test_party_messages_only_to_party() {
        let mut mgr = ChatManager::new();
        mgr.player_online(10001);
        mgr.player_online(10002);
        mgr.join_party(10001, 99901);
        mgr.join_party(10002, 99901);
        let receivers = mgr.send_party(10001, 99901, "队伍消息").unwrap();
        assert!(receivers.contains(&10002));
        assert!(!receivers.contains(&99999));
    }

    #[test]
    fn test_leave_channel() {
        let mut mgr = ChatManager::new();
        mgr.player_online(10001);
        mgr.ensure_world_channel();
        mgr.join_channel(10001, "world").unwrap();
        assert_eq!(mgr.channel_member_count("world"), 1);
        mgr.leave_channel(10001, "world");
        assert_eq!(mgr.channel_member_count("world"), 0);
    }
}
