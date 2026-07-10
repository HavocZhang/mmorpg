//! ReadLoop 读循环模块
//!
//! 独立异步任务，负责：
//! 1. 从 TCP ReadHalf 读取数据
//! 2. 喂入 PacketDecoder 解码
//! 3. 将解码后的消息路由至对应逻辑分片服务（通过 gRPC upstream）
//! 4. 更新会话活跃时间

use tokio::net::tcp::OwnedReadHalf;
use tracing::{debug, warn};

use crate::foundation::GateError;
use crate::protocol::decoder::PacketDecoder;

/// 读循环
pub struct ReadLoop {
    decoder: PacketDecoder,
    read_half: OwnedReadHalf,
}

impl ReadLoop {
    pub fn new(decoder: PacketDecoder, read_half: OwnedReadHalf) -> Self {
        Self { decoder, read_half }
    }

    /// 运行读循环
    ///
    /// 持续读取数据直到连接关闭或发生错误
    pub async fn run<F>(&mut self, mut on_packet: F) -> Result<(), GateError>
    where
        F: FnMut(u16, Vec<u8>),
    {
        let mut buf = vec![0u8; 8192 * 2];

        loop {
            let n = self.read_half.read(&mut buf).await?;

            if n == 0 {
                // 连接关闭
                debug!("对端关闭连接");
                return Ok(());
            }

            // 喂入解码器
            self.decoder.feed(&buf[..n]);

            // 解码所有完整包
            match self.decoder.decode_all() {
                Ok(packets) => {
                    for (packet, payload) in packets {
                        on_packet(packet.header.msg_id, payload);
                    }
                }
                Err(e) => {
                    warn!("解码错误: {}", e);
                    return Err(e);
                }
            }
        }
    }
}

use tokio::io::AsyncReadExt;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::aes_gcm::AesGcmCipher;
    use crate::protocol::encoder::PacketEncoder;

    const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    #[test]
    fn test_readloop_struct() {
        // 结构体创建测试（需要真实TCP连接才能完整测试）
        let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
        let decoder = PacketDecoder::new(cipher);
        // ReadLoop::new 需要 OwnedReadHalf，这里只验证类型存在
    }
}
