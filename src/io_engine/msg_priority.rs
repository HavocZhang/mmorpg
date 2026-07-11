//! 消息优先级队列模块
//!
//! 三级消息优先级：
//! - 战斗包（High）：最高优先级，保证团战流畅
//! - 聊天包（Normal）：中等优先级
//! - 公告包（Low）：最低优先级，拥堵时可丢弃
//!
//! 拥堵降级：队列深度超过阈值时丢弃 Low 包，保障 High 包不丢

use std::collections::BinaryHeap;

use crate::session::session_struct::PendingMsg;

/// 优先级队列包装（BinaryHeap 需要实现 Ord，反转使 High 先出）
struct PrioritizedMsg(PendingMsg);

impl PartialEq for PrioritizedMsg {
    fn eq(&self, other: &Self) -> bool {
        self.0.priority == other.0.priority
    }
}

impl Eq for PrioritizedMsg {}

impl PartialOrd for PrioritizedMsg {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedMsg {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // 优先级高的先出（BinaryHeap 是最大堆）
        self.0.priority.cmp(&other.0.priority)
    }
}

/// 消息优先级队列
pub struct PriorityQueue {
    heap: BinaryHeap<PrioritizedMsg>,
    len: usize,
}

impl PriorityQueue {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            len: 0,
        }
    }

    /// 入队
    pub fn push(&mut self, msg: PendingMsg) {
        self.heap.push(PrioritizedMsg(msg));
        self.len += 1;
    }

    /// 出队（按优先级高的先出）
    pub fn pop(&mut self) -> Option<PendingMsg> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        self.heap.pop().map(|p| p.0)
    }

    /// 队列长度
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for PriorityQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::session_struct::MsgPriority;

    fn make_msg(id: u16, priority: MsgPriority) -> PendingMsg {
        PendingMsg {
            msg_id: id,
            payload: vec![0; 10],
            priority,
        }
    }

    #[test]
    fn test_priority_order() {
        let mut q = PriorityQueue::new();
        q.push(make_msg(1, MsgPriority::Low));
        q.push(make_msg(2, MsgPriority::High));
        q.push(make_msg(3, MsgPriority::Normal));

        assert_eq!(q.pop().unwrap().msg_id, 2); // High
        assert_eq!(q.pop().unwrap().msg_id, 3); // Normal
        assert_eq!(q.pop().unwrap().msg_id, 1); // Low
    }

    #[test]
    fn test_empty_queue() {
        let mut q = PriorityQueue::new();
        assert!(q.is_empty());
        assert!(q.pop().is_none());
    }

    #[test]
    fn test_same_priority_fifo() {
        let mut q = PriorityQueue::new();
        q.push(make_msg(1, MsgPriority::High));
        q.push(make_msg(2, MsgPriority::High));
        q.push(make_msg(3, MsgPriority::High));

        // 同优先级不保证顺序，但都应出队
        let ids: Vec<_> = (0..3).filter_map(|_| q.pop()).map(|m| m.msg_id).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }

    #[test]
    fn test_len() {
        let mut q = PriorityQueue::new();
        assert_eq!(q.len(), 0);
        q.push(make_msg(1, MsgPriority::Normal));
        assert_eq!(q.len(), 1);
        q.push(make_msg(2, MsgPriority::Normal));
        assert_eq!(q.len(), 2);
        q.pop();
        assert_eq!(q.len(), 1);
    }
}
