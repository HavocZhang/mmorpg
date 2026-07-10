//! 雪花ID生成器模块
//!
//! 用于生成全局唯一 session_id
//! 结构：[1bit符号位][41bit时间戳][10bit机器ID][12bit序列号]
//! 单机每秒可生成 4096*1000 = ~400万 ID

use parking_lot::Mutex;
use thiserror::Error;

const EPOCH: u64 = 1_704_067_200_000; // 2024-01-01 00:00:00 UTC
const MACHINE_ID_BITS: u8 = 10;
const SEQUENCE_BITS: u8 = 12;

const MAX_MACHINE_ID: u64 = (1 << MACHINE_ID_BITS) - 1;
const MAX_SEQUENCE: u64 = (1 << SEQUENCE_BITS) - 1;

const MACHINE_ID_SHIFT: u8 = SEQUENCE_BITS;
const TIMESTAMP_SHIFT: u8 = SEQUENCE_BITS + MACHINE_ID_BITS;

#[derive(Debug, Error)]
pub enum SnowflakeError {
    #[error("机器ID超出范围: {0} > {max}", max = MAX_MACHINE_ID)]
    MachineIdOutOfRange(u64),
    #[error("系统时钟回拨")]
    ClockMovedBackwards,
}

/// 雪花ID生成器
pub struct SnowflakeIdGen {
    machine_id: u64,
    last_timestamp: u64,
    sequence: u64,
    inner: Mutex<()>,
}

impl SnowflakeIdGen {
    /// 创建ID生成器
    ///
    /// # 参数
    /// - `machine_id`：机器ID，范围 0..1023
    pub fn new(machine_id: u64) -> Result<Self, SnowflakeError> {
        if machine_id > MAX_MACHINE_ID {
            return Err(SnowflakeError::MachineIdOutOfRange(machine_id));
        }
        Ok(Self {
            machine_id,
            last_timestamp: 0,
            sequence: 0,
            inner: Mutex::new(()),
        })
    }

    /// 生成下一个唯一ID
    pub fn next_id(&mut self) -> Result<u64, SnowflakeError> {
        let _guard = self.inner.lock();
        let mut now = current_timestamp();

        if now < self.last_timestamp {
            return Err(SnowflakeError::ClockMovedBackwards);
        }

        if now == self.last_timestamp {
            self.sequence = (self.sequence + 1) & MAX_SEQUENCE;
            if self.sequence == 0 {
                // 当前毫秒序列号用尽，等待下一毫秒
                while now == self.last_timestamp {
                    now = current_timestamp();
                }
            }
        } else {
            self.sequence = 0;
        }

        self.last_timestamp = now;

        Ok(
            ((now - EPOCH) << TIMESTAMP_SHIFT)
                | (self.machine_id << MACHINE_ID_SHIFT)
                | self.sequence,
        )
    }
}

/// 获取当前时间戳（毫秒，相对于epoch）
fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_machine_id_range() {
        assert!(SnowflakeIdGen::new(MAX_MACHINE_ID + 1).is_err());
        assert!(SnowflakeIdGen::new(0).is_ok());
        assert!(SnowflakeIdGen::new(MAX_MACHINE_ID).is_ok());
    }

    #[test]
    fn test_id_uniqueness() {
        let mut gen = SnowflakeIdGen::new(1).unwrap();
        let mut ids = HashSet::new();
        for _ in 0..10000 {
            let id = gen.next_id().unwrap();
            assert!(ids.insert(id), "生成重复ID: {}", id);
        }
    }

    #[test]
    fn test_id_concurrent_uniqueness() {
        use std::sync::Arc;
        use std::thread;

        let gen = Arc::new(parking_lot::Mutex::new(SnowflakeIdGen::new(1).unwrap()));
        let mut handles = vec![];

        for _ in 0..4 {
            let g = gen.clone();
            handles.push(thread::spawn(move || {
                let mut local_ids = vec![];
                for _ in 0..1000 {
                    let id = g.lock().next_id().unwrap();
                    local_ids.push(id);
                }
                local_ids
            }));
        }

        let mut all_ids = HashSet::new();
        for h in handles {
            for id in h.join().unwrap() {
                assert!(all_ids.insert(id), "并发生成重复ID: {}", id);
            }
        }
    }
}
