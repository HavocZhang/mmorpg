//! TDD 单元测试 — IO引擎模块（读循环、写循环、小包合并、优先级队列）
//!
//! 测试 PacketMerge、MsgPriority 队列、合并窗口机制

use std::time::Duration;
use rust_mmo_gate::crypto::aes_gcm::AesGcmCipher;
use rust_mmo_gate::io_engine::packet_merge::PacketMerge;
use rust_mmo_gate::io_engine::msg_priority::PriorityQueue;
use rust_mmo_gate::session::session_struct::{MsgPriority, PendingMsg};

const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn make_cipher() -> AesGcmCipher {
    AesGcmCipher::from_hex_key(TEST_KEY).unwrap()
}

fn make_msg(id: u16, size: usize, priority: MsgPriority) -> PendingMsg {
    PendingMsg { msg_id: id, payload: vec![0xAB; size], priority }
}

// ── PacketMerge 测试 ──

#[test]
fn test_packet_merge_single_packet() {
    let mut merge = PacketMerge::new(Duration::from_millis(16), make_cipher());
    merge.push(make_msg(1, 10, MsgPriority::Normal));
    assert_eq!(merge.pending_count(), 1);
    let flushed = merge.flush();
    assert!(flushed.is_some());
}

#[test]
fn test_packet_merge_multiple_packets() {
    let mut merge = PacketMerge::new(Duration::from_millis(16), make_cipher());
    for i in 0..10 {
        merge.push(make_msg(i, 50, MsgPriority::Normal));
    }
    assert_eq!(merge.pending_count(), 10);
    let data = merge.flush().unwrap();
    assert!(data.len() > 10 * 50, "合并后数据应大于原始数据总和");
}

#[test]
fn test_packet_merge_empty_flush() {
    let mut merge = PacketMerge::new(Duration::from_millis(16), make_cipher());
    assert!(merge.flush().is_none());
    assert_eq!(merge.pending_count(), 0);
}

#[test]
fn test_packet_merge_window_not_expired() {
    let mut merge = PacketMerge::new(Duration::from_secs(60), make_cipher());
    merge.push(make_msg(1, 10, MsgPriority::Normal));
    assert!(merge.try_flush().is_none(), "窗口未到不应刷新");
}

#[test]
fn test_packet_merge_window_expired() {
    let mut merge = PacketMerge::new(Duration::from_millis(1), make_cipher());
    merge.push(make_msg(1, 10, MsgPriority::Normal));
    std::thread::sleep(Duration::from_millis(5));
    assert!(merge.try_flush().is_some(), "窗口过期应刷新");
}

#[test]
fn test_packet_merge_reset_after_flush() {
    let mut merge = PacketMerge::new(Duration::from_millis(16), make_cipher());
    merge.push(make_msg(1, 10, MsgPriority::Normal));
    merge.flush();
    assert_eq!(merge.pending_count(), 0);
    assert!(merge.flush().is_none());
}

// ── PriorityQueue 测试 ──

#[test]
fn test_priority_queue_ordering() {
    let mut q = PriorityQueue::new();
    q.push(make_msg(1, 10, MsgPriority::Low));
    q.push(make_msg(2, 10, MsgPriority::High));
    q.push(make_msg(3, 10, MsgPriority::Normal));
    
    let first = q.pop().unwrap();
    assert_eq!(first.priority, MsgPriority::High, "高优先级应先出");
    let second = q.pop().unwrap();
    assert_eq!(second.priority, MsgPriority::Normal);
    let third = q.pop().unwrap();
    assert_eq!(third.priority, MsgPriority::Low);
}

#[test]
fn test_priority_queue_empty_pop() {
    let mut q: PriorityQueue = PriorityQueue::new();
    assert!(q.pop().is_none());
}

#[test]
fn test_priority_queue_multiple_same_priority() {
    let mut q = PriorityQueue::new();
    for i in 0..5 {
        q.push(make_msg(i, 10, MsgPriority::High));
    }
    for _i in 0..5 {
        let msg = q.pop().unwrap();
        assert_eq!(msg.priority, MsgPriority::High);
    }
    assert!(q.pop().is_none());
}

#[test]
fn test_priority_queue_large_volume() {
    let mut q = PriorityQueue::new();
    for i in 0..1000 {
        let prio = match i % 3 {
            0 => MsgPriority::High,
            1 => MsgPriority::Normal,
            _ => MsgPriority::Low,
        };
        q.push(make_msg(i as u16, 10, prio));
    }
    let mut prev = MsgPriority::High;
    let mut count = 0;
    while let Some(msg) = q.pop() {
        assert!(msg.priority <= prev, "优先级应降序");
        prev = msg.priority;
        count += 1;
    }
    assert_eq!(count, 1000);
}
