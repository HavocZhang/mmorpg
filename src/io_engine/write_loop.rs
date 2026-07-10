//! WriteLoop 写循环模块
//!
//! 独立异步任务，负责：
//! 1. 从发送通道读取待发消息
//! 2. 16ms 滑动窗口合并小包，减少系统调用
//! 3. 三级消息优先级排序
//! 4. 网络拥堵时丢弃低优先级包
//! 5. 通过 TCP WriteHalf 发送

use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::time::{interval, Instant};
use tracing::{debug, warn};

use crate::crypto::aes_gcm::AesGcmCipher;
use crate::session::session_struct::{MsgPriority, PendingMsg};

/// 写循环
pub struct WriteLoop {
    write_half: OwnedWriteHalf,
    merge_window: Duration,
    /// 最大队列深度（超过则丢弃低优先级包）
    max_queue_depth: usize,
    /// 加密器（用于下行包加密）
    cipher: AesGcmCipher,
}

impl WriteLoop {
    pub fn new(write_half: OwnedWriteHalf, merge_window_ms: u64, cipher: AesGcmCipher) -> Self {
        Self {
            write_half,
            merge_window: Duration::from_millis(merge_window_ms),
            max_queue_depth: 1024,
            cipher,
        }
    }

    /// 运行写循环
    ///
    /// 从 rx 通道读取消息，按合并窗口和优先级处理后发送
    pub async fn run(
        &mut self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<PendingMsg>,
    ) -> Result<(), std::io::Error> {
        let mut merge = super::packet_merge::PacketMerge::new(self.merge_window, self.cipher.clone());
        let mut priority_q = super::msg_priority::PriorityQueue::new();
        let mut flush_timer = interval(self.merge_window);

        loop {
            tokio::select! {
                // 收到新消息
                msg = rx.recv() => {
                    match msg {
                        Some(msg) => {
                            // 拥堵检查：队列过深时丢弃低优先级包
                            if priority_q.len() > self.max_queue_depth
                                && msg.priority == MsgPriority::Low {
                                warn!("队列拥堵，丢弃低优先级包 msg_id={}", msg.msg_id);
                                continue;
                            }
                            priority_q.push(msg);
                        }
                        None => {
                            // 通道关闭，退出循环
                            debug!("发送通道关闭，WriteLoop 退出");
                            break;
                        }
                    }
                }
                // 合并窗口触发
                _ = flush_timer.tick() => {
                    // 按优先级取出所有消息
                    while let Some(msg) = priority_q.pop() {
                        merge.push(msg);
                    }
                    // 合并发送
                    if let Some(data) = merge.flush() {
                        self.write_half.write_all(&data).await?;
                    }
                }
            }
        }

        // 刷新剩余消息
        while let Some(msg) = priority_q.pop() {
            merge.push(msg);
        }
        if let Some(data) = merge.flush() {
            self.write_half.write_all(&data).await?;
        }

        Ok(())
    }
}
