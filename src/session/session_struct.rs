//! 会话结构定义
//!
//! 每个在线玩家对应一个 Session

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc;

use crate::protocol::packet_struct::MsgId;

/// 消息优先级
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MsgPriority {
    /// 公告/系统消息（最低）
    Low = 0,
    /// 聊天/社交消息
    Normal = 1,
    /// 战斗包（最高）
    High = 2,
}

/// 待发送消息
#[derive(Clone)]
pub struct PendingMsg {
    pub msg_id: MsgId,
    pub payload: Vec<u8>,
    pub priority: MsgPriority,
}

/// 会话状态
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// 握手中
    Handshaking,
    /// 在线
    Online,
    /// 正在关闭
    Closing,
    /// 已关闭
    Closed,
}

/// 会话结构
pub struct Session {
    /// 会话唯一ID（雪花ID生成）
    pub session_id: u64,
    /// 玩家UID
    pub player_uid: AtomicU64,
    /// 客户端地址
    pub peer_addr: SocketAddr,
    /// 会话状态
    pub state: parking_lot::RwLock<SessionState>,
    /// 最后活跃时间（心跳更新）
    pub last_active: parking_lot::RwLock<Instant>,
    /// 创建时间
    pub created_at: Instant,
    /// 发送通道（WriteLoop 从此通道读取消息）
    pub send_tx: mpsc::UnboundedSender<PendingMsg>,
    /// 是否活跃
    pub active: AtomicBool,
}

impl Session {
    /// 创建新会话
    pub fn new(
        session_id: u64,
        peer_addr: SocketAddr,
        player_uid: u64,
        send_tx: mpsc::UnboundedSender<PendingMsg>,
    ) -> Self {
        let now = Instant::now();
        Self {
            session_id,
            player_uid: AtomicU64::new(player_uid),
            peer_addr,
            state: parking_lot::RwLock::new(SessionState::Online),
            last_active: parking_lot::RwLock::new(now),
            created_at: now,
            send_tx,
            active: AtomicBool::new(true),
        }
    }

    /// 更新最后活跃时间
    pub fn touch(&self) {
        *self.last_active.write() = Instant::now();
    }

    /// 获取玩家UID
    pub fn player_uid(&self) -> u64 {
        self.player_uid.load(Ordering::Relaxed)
    }

    /// 获取会话状态
    pub fn state(&self) -> SessionState {
        *self.state.read()
    }

    /// 设置会话状态
    pub fn set_state(&self, state: SessionState) {
        *self.state.write() = state;
    }

    /// 是否在线
    pub fn is_online(&self) -> bool {
        self.state() == SessionState::Online
    }

    /// 是否空闲超时
    pub fn is_idle_timeout(&self, timeout_secs: u64) -> bool {
        let last = *self.last_active.read();
        last.elapsed().as_millis() as u64 > timeout_secs * 1000
    }

    /// 发送消息（非阻塞，写入通道）
    pub fn send(&self, msg: PendingMsg) -> Result<(), GateError> {
        if !self.is_online() {
            return Err(GateError::SessionClosed(self.session_id));
        }
        self.send_tx
            .send(msg)
            .map_err(|_| GateError::SessionClosed(self.session_id))
    }

    /// 标记为关闭
    pub fn close(&self) {
        self.active.store(false, Ordering::Relaxed);
        self.set_state(SessionState::Closed);
    }
}

use crate::foundation::GateError;

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("session_id", &self.session_id)
            .field("player_uid", &self.player_uid())
            .field("peer_addr", &self.peer_addr)
            .field("state", &self.state())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_session() -> (Arc<Session>, mpsc::UnboundedReceiver<PendingMsg>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let session = Arc::new(Session::new(
            1,
            "127.0.0.1:12345".parse().unwrap(),
            100,
            tx,
        ));
        (session, rx)
    }

    #[test]
    fn test_session_creation() {
        let (session, _rx) = create_test_session();
        assert_eq!(session.session_id, 1);
        assert_eq!(session.player_uid(), 100);
        assert!(session.is_online());
    }

    #[test]
    fn test_session_state_transition() {
        let (session, _rx) = create_test_session();
        assert_eq!(session.state(), SessionState::Online);
        session.set_state(SessionState::Closing);
        assert_eq!(session.state(), SessionState::Closing);
        session.set_state(SessionState::Closed);
        assert!(!session.is_online());
    }

    #[test]
    fn test_session_send() {
        let (session, mut rx) = create_test_session();
        let msg = PendingMsg {
            msg_id: 1,
            payload: vec![1, 2, 3],
            priority: MsgPriority::Normal,
        };
        assert!(session.send(msg).is_ok());
        let received = rx.try_recv().unwrap();
        assert_eq!(received.msg_id, 1);
    }

    #[test]
    fn test_session_send_after_close() {
        let (session, _rx) = create_test_session();
        session.close();
        let msg = PendingMsg {
            msg_id: 1,
            payload: vec![1],
            priority: MsgPriority::Normal,
        };
        assert!(session.send(msg).is_err());
    }

    #[test]
    fn test_session_touch() {
        let (session, _rx) = create_test_session();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!session.is_idle_timeout(100));
        assert!(session.is_idle_timeout(0));
    }
}
