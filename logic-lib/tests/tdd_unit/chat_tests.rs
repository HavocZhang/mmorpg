//! 聊天服 TDD 单元测试 — 测试 src/chat 模块

use logic_lib::chat::ChatManager;

#[test]
fn test_channel_creation() {
    let mut mgr = ChatManager::new();
    mgr.ensure_world_channel();
    assert_eq!(mgr.channel_member_count("world"), 0);
    assert_eq!(mgr.channel_message_count("world"), 0);
}

#[test]
fn test_message_send_to_channel() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    mgr.ensure_world_channel();
    mgr.join_channel(10001, "world").unwrap();
    let result = mgr.send_to_channel(10001, "world", "你好世界");
    assert!(result.is_ok());
    let receivers = result.unwrap();
    assert!(receivers.contains(&10001));
    assert_eq!(mgr.channel_message_count("world"), 1);
    assert_eq!(mgr.acks.len(), 1);
}

#[test]
fn test_private_message_targeting() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    let result = mgr.send_private(10001, 10002, "私下消息");
    assert!(result.is_ok());
    let pms = mgr.get_private_messages();
    assert_eq!(pms.len(), 1);
    assert_eq!(pms[0], (10001, 10002, "私下消息".to_string()));
}

#[test]
fn test_private_message_not_sent_to_others() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    mgr.send_private(10001, 10002, "only for 10002").unwrap();
    // 10003 should not receive
    let pms = mgr.get_private_messages();
    let has_10003 = pms.iter().any(|(_, t, _)| *t == 10003);
    assert!(!has_10003);
}

#[test]
fn test_message_history_retrieval() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    mgr.ensure_world_channel();
    mgr.join_channel(10001, "world").unwrap();
    mgr.send_to_channel(10001, "world", "消息1").unwrap();
    mgr.send_to_channel(10001, "world", "消息2").unwrap();
    mgr.send_to_channel(10001, "world", "消息3").unwrap();
    let history = mgr.query_history("world", 10);
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].1, "消息1");
    assert_eq!(history[2].1, "消息3");
}

#[test]
fn test_history_limit() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    mgr.ensure_world_channel();
    mgr.join_channel(10001, "world").unwrap();
    for i in 0..5 {
        mgr.send_to_channel(10001, "world", &format!("msg{}", i)).unwrap();
    }
    let history = mgr.query_history("world", 2);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].1, "msg3");
    assert_eq!(history[1].1, "msg4");
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
    let third = mgr.send_to_channel(10001, "world", "m3");
    assert!(third.is_err());
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
    assert!(mgr.send_to_channel(10001, "world", &long_text).is_err());
}

#[test]
fn test_message_within_length_limit() {
    let mut mgr = ChatManager::new();
    mgr.set_max_message_length(200);
    mgr.player_online(10001);
    mgr.ensure_world_channel();
    mgr.join_channel(10001, "world").unwrap();
    let ok_text = "a".repeat(200);
    assert!(mgr.send_to_channel(10001, "world", &ok_text).is_ok());
}

#[test]
fn test_keyword_filtering() {
    let mut mgr = ChatManager::new();
    mgr.add_sensitive_word("外挂");
    mgr.add_sensitive_word("广告");
    mgr.player_online(10001);
    mgr.ensure_world_channel();
    mgr.join_channel(10001, "world").unwrap();
    assert!(mgr.send_to_channel(10001, "world", "加微信买外挂").is_err());
    assert_eq!(mgr.filtered_count, 1);
    assert_eq!(mgr.channel_message_count("world"), 0);
}

#[test]
fn test_keyword_filtering_multiple_words() {
    let mut mgr = ChatManager::new();
    mgr.add_sensitive_word("外挂");
    mgr.add_sensitive_word("私服");
    mgr.player_online(10001);
    mgr.ensure_world_channel();
    mgr.join_channel(10001, "world").unwrap();
    assert!(mgr.send_to_channel(10001, "world", "来玩私服").is_err());
    assert!(mgr.send_to_channel(10001, "world", "正常聊天").is_ok());
    assert_eq!(mgr.filtered_count, 1);
    assert_eq!(mgr.channel_message_count("world"), 1);
}

#[test]
fn test_clean_message_passes_filter() {
    let mut mgr = ChatManager::new();
    mgr.add_sensitive_word("外挂");
    mgr.player_online(10001);
    mgr.ensure_world_channel();
    mgr.join_channel(10001, "world").unwrap();
    assert!(mgr.send_to_channel(10001, "world", "你好啊").is_ok());
}

#[test]
fn test_offline_message_queue() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    // 10002 is not online
    mgr.send_private(10001, 10002, "离线消息1").unwrap();
    mgr.send_private(10001, 10002, "离线消息2").unwrap();
    assert_eq!(mgr.offline_message_count(10002), 2);
    let msgs = mgr.get_offline_messages(10002);
    assert_eq!(msgs[0].1, "离线消息1");
    assert_eq!(msgs[1].1, "离线消息2");
}

#[test]
fn test_no_offline_for_online_player() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    mgr.player_online(10002);
    mgr.send_private(10001, 10002, "在线消息").unwrap();
    assert_eq!(mgr.offline_message_count(10002), 0);
}

#[test]
fn test_multi_channel_subscription() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    mgr.ensure_world_channel();
    mgr.create_channel("guild_chat", logic_lib::chat::ChannelType::Guild("test_guild".to_string()));
    mgr.create_channel("party_chat", logic_lib::chat::ChannelType::Party(99));
    mgr.join_channel(10001, "world").unwrap();
    mgr.join_channel(10001, "guild_chat").unwrap();
    mgr.join_channel(10001, "party_chat").unwrap();
    assert_eq!(mgr.channel_member_count("world"), 1);
    assert_eq!(mgr.channel_member_count("guild_chat"), 1);
    assert_eq!(mgr.channel_member_count("party_chat"), 1);
}

#[test]
fn test_broadcast_efficiency() {
    let mut mgr = ChatManager::new();
    mgr.ensure_world_channel();
    for i in 1..=100u64 {
        mgr.player_online(i);
        mgr.join_channel(i, "world").unwrap();
    }
    // Send one message should return all 100 members
    assert_eq!(mgr.channel_member_count("world"), 100);
    let receivers = mgr.send_to_channel(1, "world", "广播测试").unwrap();
    assert_eq!(receivers.len(), 100);
}

#[test]
fn test_player_leave_channel() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    mgr.ensure_world_channel();
    mgr.join_channel(10001, "world").unwrap();
    assert_eq!(mgr.channel_member_count("world"), 1);
    mgr.leave_channel(10001, "world");
    assert_eq!(mgr.channel_member_count("world"), 0);
}

#[test]
fn test_guild_message_only_to_guild_members() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    mgr.player_online(10002);
    mgr.player_online(10003);
    mgr.join_guild(10001, "屠龙公会");
    mgr.join_guild(10002, "屠龙公会");
    // 10003 is NOT in guild
    let receivers = mgr.send_guild(10001, "屠龙公会", "公会消息").unwrap();
    assert!(receivers.contains(&10002));
    assert!(!receivers.contains(&10003));
    assert_eq!(mgr.acks.len(), 1);
}

#[test]
fn test_party_message_only_to_party_members() {
    let mut mgr = ChatManager::new();
    mgr.player_online(10001);
    mgr.player_online(10002);
    mgr.player_online(10003);
    mgr.join_party(10001, 99901);
    mgr.join_party(10002, 99901);
    // 10003 is NOT in party
    let receivers = mgr.send_party(10001, 99901, "队伍消息").unwrap();
    assert!(receivers.contains(&10002));
    assert!(!receivers.contains(&10003));
}
