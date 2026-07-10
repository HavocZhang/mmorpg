//! 小包合并模块
//!
//! 16ms 滑动窗口内累积小包，合并为一个大包发送
//! 减少系统调用次数，提升团战场景吞吐量
//! 目标：小包合并压缩率 ≥ 70%

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::crypto::aes_gcm::AesGcmCipher;
use crate::protocol::packet_struct::Packet;
use crate::session::session_struct::PendingMsg;

/// 全局合包统计（所有 WriteLoop 共享）
pub static MERGE_TOTAL_PACKETS: AtomicU64 = AtomicU64::new(0);
pub static MERGE_TOTAL_FLUSHES: AtomicU64 = AtomicU64::new(0);
pub static MERGE_TOTAL_BYTES_SENT: AtomicU64 = AtomicU64::new(0);

/// 滑动窗口快照（用于计算近实时压缩率）
/// 每次 merge_stats_with_recent() 被调用时，记录当前累积值和时间戳
/// 下次调用时通过差值计算近实时压缩率
static SNAPSHOT_PACKETS: AtomicU64 = AtomicU64::new(0);
static SNAPSHOT_FLUSHES: AtomicU64 = AtomicU64::new(0);
static SNAPSHOT_BYTES: AtomicU64 = AtomicU64::new(0);
static SNAPSHOT_TIME: AtomicU64 = AtomicU64::new(0);

/// 获取合包压缩率统计（累积值）
pub fn merge_stats() -> (u64, u64, f64) {
    let packets = MERGE_TOTAL_PACKETS.load(Ordering::Relaxed);
    let flushes = MERGE_TOTAL_FLUSHES.load(Ordering::Relaxed);
    let compression_rate = if packets > 0 {
        (1.0 - flushes as f64 / packets as f64) * 100.0
    } else {
        0.0
    };
    (packets, flushes, compression_rate)
}

/// 获取合包统计（累积 + 近实时）
///
/// 返回 (total_packets, total_flushes, cumulative_rate,
///        recent_packets, recent_flushes, recent_rate,
///        recent_bytes_per_sec)
///
/// 近实时值基于上次调用到现在的差值，适合 Prometheus 定期抓取
pub fn merge_stats_with_recent() -> MergeStatsSnapshot {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let total_packets = MERGE_TOTAL_PACKETS.load(Ordering::Relaxed);
    let total_flushes = MERGE_TOTAL_FLUSHES.load(Ordering::Relaxed);
    let total_bytes = MERGE_TOTAL_BYTES_SENT.load(Ordering::Relaxed);

    let prev_packets = SNAPSHOT_PACKETS.load(Ordering::Relaxed);
    let prev_flushes = SNAPSHOT_FLUSHES.load(Ordering::Relaxed);
    let prev_bytes = SNAPSHOT_BYTES.load(Ordering::Relaxed);
    let prev_time = SNAPSHOT_TIME.load(Ordering::Relaxed);

    // 计算累积压缩率
    let cumulative_rate = if total_packets > 0 {
        (1.0 - total_flushes as f64 / total_packets as f64) * 100.0
    } else {
        0.0
    };

    // 计算近实时压缩率
    let recent_packets = total_packets.saturating_sub(prev_packets);
    let recent_flushes = total_flushes.saturating_sub(prev_flushes);
    let recent_bytes = total_bytes.saturating_sub(prev_bytes);
    let elapsed = now.saturating_sub(prev_time);

    let recent_rate = if recent_packets > 0 {
        (1.0 - recent_flushes as f64 / recent_packets as f64) * 100.0
    } else {
        0.0
    };

    let bytes_per_sec = if elapsed > 0 {
        recent_bytes / elapsed
    } else {
        0
    };

    // 更新快照
    SNAPSHOT_PACKETS.store(total_packets, Ordering::Relaxed);
    SNAPSHOT_FLUSHES.store(total_flushes, Ordering::Relaxed);
    SNAPSHOT_BYTES.store(total_bytes, Ordering::Relaxed);
    SNAPSHOT_TIME.store(now, Ordering::Relaxed);

    MergeStatsSnapshot {
        total_packets,
        total_flushes,
        total_bytes,
        cumulative_rate,
        recent_packets,
        recent_flushes,
        recent_rate,
        bytes_per_sec,
        elapsed_secs: elapsed,
    }
}

/// 合包统计快照
pub struct MergeStatsSnapshot {
    pub total_packets: u64,
    pub total_flushes: u64,
    pub total_bytes: u64,
    pub cumulative_rate: f64,
    pub recent_packets: u64,
    pub recent_flushes: u64,
    pub recent_rate: f64,
    pub bytes_per_sec: u64,
    pub elapsed_secs: u64,
}

/// 重置统计（测试用）
pub fn reset_merge_stats() {
    MERGE_TOTAL_PACKETS.store(0, Ordering::Relaxed);
    MERGE_TOTAL_FLUSHES.store(0, Ordering::Relaxed);
    MERGE_TOTAL_BYTES_SENT.store(0, Ordering::Relaxed);
    SNAPSHOT_PACKETS.store(0, Ordering::Relaxed);
    SNAPSHOT_FLUSHES.store(0, Ordering::Relaxed);
    SNAPSHOT_BYTES.store(0, Ordering::Relaxed);
    SNAPSHOT_TIME.store(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        Ordering::Relaxed,
    );
}

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
        MERGE_TOTAL_PACKETS.fetch_add(1, Ordering::Relaxed);
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
        MERGE_TOTAL_FLUSHES.fetch_add(1, Ordering::Relaxed);
        MERGE_TOTAL_BYTES_SENT.fetch_add(data.len() as u64, Ordering::Relaxed);
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
