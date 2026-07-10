//! 小包合并模块
//!
//! 16ms 滑动窗口内累积小包，合并为一个大包发送
//! 减少系统调用次数，提升团战场景吞吐量
//! 目标：小包合并压缩率 ≥ 70%

use std::time::{Duration, Instant};

use crate::crypto::aes_gcm::AesGcmCipher;
use crate::protocol::packet_struct::Packet;
use crate::session::session_struct::PendingMsg;

/// 小包合并器
pub struct PacketMerge {
    /// 合并窗口
    window: Duration,
    /// 窗口开始时间
    window_start: Option<Instant>,
    /// 当前窗口内待合并数据
    pending: Vec<u8>,
    /// 当前窗口内包数
    packet_count: usize,
    /// 加密器（用于下行包加密）
    cipher: AesGcmCipher,
}

impl PacketMerge {
    pub fn new(window: Duration, cipher: AesGcmCipher) -> Self {
        Self {
            window,
            window_start: None,
            pending: Vec::with_capacity(8192),
            packet_count: 0,
            cipher,
        }
    }

    /// 添加一个待发送消息
    pub fn push(&mut self, msg: PendingMsg) {
        if self.window_start.is_none() {
            self.window_start = Some(Instant::now());
        }

        // 加密 payload，构建完整协议包（16字节头 + 加密体）
        let encrypted = self.cipher.encrypt(&msg.payload).unwrap_or_else(|_| {
            // 加密失败时使用原始数据（不应发生，仅防御性处理）
            msg.payload.clone()
        });
        let packet = Packet::new(msg.msg_id, encrypted);
        let packet_bytes = packet.to_bytes();

        self.pending.extend_from_slice(&packet_bytes);
        self.packet_count += 1;
    }

    /// 尝试刷新（如果窗口已满或手动触发）
    pub fn try_flush(&mut self) -> Option<Vec<u8>> {
        if self.packet_count == 0 {
            return None;
        }

        if let Some(start) = self.window_start {
            if start.elapsed() >= self.window {
                return self.flush();
            }
        }
        None
    }

    /// 强制刷新，返回合并数据
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.packet_count == 0 {
            return None;
        }
        let data = std::mem::take(&mut self.pending);
        self.packet_count = 0;
        self.window_start = None;
        Some(data)
    }

    /// 当前窗口内包数
    pub fn pending_count(&self) -> usize {
        self.packet_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::session_struct::MsgPriority;

    const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn make_cipher() -> AesGcmCipher {
        AesGcmCipher::from_hex_key(TEST_KEY).unwrap()
    }

    fn make_msg(id: u16, size: usize) -> PendingMsg {
        PendingMsg {
            msg_id: id,
            payload: vec![0xAB; size],
            priority: MsgPriority::Normal,
        }
    }

    #[test]
    fn test_merge_single_packet() {
        let mut merge = PacketMerge::new(Duration::from_millis(16), make_cipher());
        merge.push(make_msg(1, 10));
        let data = merge.flush();
        assert!(data.is_some());
        assert!(data.unwrap().len() > 10);
    }

    #[test]
    fn test_merge_multiple_packets() {
        let mut merge = PacketMerge::new(Duration::from_millis(16), make_cipher());
        for i in 0..10 {
            merge.push(make_msg(i, 50));
        }
        assert_eq!(merge.pending_count(), 10);
        let data = merge.flush().unwrap();
        // 每包: 16(header) + 12(nonce) + 50(ciphertext) + 16(tag) = 94 bytes
        assert_eq!(data.len(), 10 * 94);
    }

    #[test]
    fn test_merge_empty_flush() {
        let mut merge = PacketMerge::new(Duration::from_millis(16), make_cipher());
        assert!(merge.flush().is_none());
    }

    #[test]
    fn test_merge_try_flush_before_window() {
        let mut merge = PacketMerge::new(Duration::from_secs(60), make_cipher());
        merge.push(make_msg(1, 10));
        // 窗口未到，不应刷新
        assert!(merge.try_flush().is_none());
    }

    #[test]
    fn test_merge_try_flush_after_window() {
        let mut merge = PacketMerge::new(Duration::from_millis(1), make_cipher());
        merge.push(make_msg(1, 10));
        std::thread::sleep(Duration::from_millis(5));
        assert!(merge.try_flush().is_some());
    }
}
