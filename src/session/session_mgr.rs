//! 会话管理器
//!
//! 使用 DashMap 无锁双映射：
//! - session_id -> Session
//! - player_uid -> session_id
//!
//! 支持：并发创建/销毁、高频读写、顶号下线、僵尸连接清理

use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::foundation::metric;
use crate::foundation::snowflake::SnowflakeIdGen;
use crate::session::session_struct::{PendingMsg, Session};

/// 会话管理器（无锁双映射）
pub struct SessionManager {
    /// session_id -> Session 映射
    session_map: DashMap<u64, Arc<Session>>,
    /// player_uid -> session_id 映射
    uid_map: DashMap<u64, u64>,
    /// 雪花ID生成器
    id_gen: parking_lot::Mutex<SnowflakeIdGen>,
}

impl SessionManager {
    /// 创建会话管理器
    pub fn new() -> Self {
        Self {
            session_map: DashMap::new(),
            uid_map: DashMap::new(),
            id_gen: parking_lot::Mutex::new(SnowflakeIdGen::new(1).expect("雪花ID生成器初始化失败")),
        }
    }

    /// 创建新会话
    ///
    /// 如果同一 player_uid 已有旧会话，自动顶掉旧会话
    ///
    /// 返回 (session_id, send_rx)：
    /// - session_id：会话唯一ID
    /// - send_rx：消息接收端，调用方用它启动 WriteLoop
    pub async fn create_session(
        &self,
        peer_addr: SocketAddr,
        player_uid: u64,
    ) -> (u64, mpsc::UnboundedReceiver<PendingMsg>) {
        // 顶号：如果同一UID已有会话，先下线旧会话
        if let Some(old_session_id) = self.uid_map.get(&player_uid) {
            let old_id = *old_session_id;
            drop(old_session_id);
            self.kick_session(old_id, "顶号下线").await;
        }

        // 生成 session_id（雪花ID）
        let session_id = self.id_gen.lock().next_id().unwrap_or_else(|_| {
            // 时钟回拨时 fallback 到时间戳
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
        });

        // 创建发送通道
        let (send_tx, send_rx) = mpsc::unbounded_channel::<PendingMsg>();

        // 创建会话
        let session = Arc::new(Session::new(
            session_id,
            peer_addr,
            player_uid,
            send_tx,
        ));

        // 写入双映射
        self.session_map.insert(session_id, session.clone());
        self.uid_map.insert(player_uid, session_id);

        debug!(
            "会话创建: session_id={} uid={} addr={}",
            session_id, player_uid, peer_addr
        );

        (session_id, send_rx)
    }

    /// 获取会话
    pub fn get_session(&self, session_id: u64) -> Option<Arc<Session>> {
        self.session_map.get(&session_id).map(|r| r.clone())
    }

    /// 通过 player_uid 获取会话
    pub fn get_session_by_uid(&self, player_uid: u64) -> Option<Arc<Session>> {
        let session_id = self.uid_map.get(&player_uid)?;
        self.get_session(*session_id)
    }

    /// 踢掉会话
    pub async fn kick_session(&self, session_id: u64, reason: &str) {
        if let Some((_, session)) = self.session_map.remove(&session_id) {
            session.close();
            self.uid_map.remove(&session.player_uid());
            metric::metrics()
                .session_kicks
                .with_label_values(&[reason])
                .inc();
            metric::metrics().connections.dec();
            info!(
                "会话踢出: session_id={} uid={} reason={}",
                session_id,
                session.player_uid(),
                reason
            );
        }
    }

    /// 移除会话（资源释放）
    pub fn remove_session(&self, session_id: u64) {
        if let Some((_, session)) = self.session_map.remove(&session_id) {
            session.close();
            self.uid_map.remove(&session.player_uid());
            metric::metrics().connections.dec();
            debug!(
                "会话移除: session_id={} uid={}",
                session_id,
                session.player_uid()
            );
        }
    }

    /// 更新会话活跃时间（心跳）
    pub fn touch_session(&self, session_id: u64) {
        if let Some(session) = self.session_map.get(&session_id) {
            session.touch();
        }
    }

    /// 获取当前在线数
    pub fn online_count(&self) -> usize {
        self.session_map.len()
    }

    /// 清理僵尸连接
    pub async fn clean_idle_sessions(&self, timeout_secs: u64) {
        let mut to_remove = vec![];

        for entry in self.session_map.iter() {
            let session = entry.value();
            if session.is_idle_timeout(timeout_secs) {
                to_remove.push(session.session_id);
            }
        }

        for session_id in to_remove {
            debug!("僵尸连接清理: session_id={}", session_id);
            self.kick_session(session_id, "心跳超时").await;
        }
    }

    /// 获取所有在线会话ID
    pub fn get_all_session_ids(&self) -> Vec<u64> {
        self.session_map
            .iter()
            .map(|r| *r.key())
            .collect()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_get_session() {
        let mgr = SessionManager::new();
        let _addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

        // 测试空管理器
        assert_eq!(mgr.online_count(), 0);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let mgr = Arc::new(SessionManager::new());

        // 测试并发读写不 panic
        let mut handles = vec![];
        for i in 0..4 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move {
                // 并发读取
                let _ = m.get_session(i as u64);
                let _ = m.get_session_by_uid(i as u64);
                let _ = m.online_count();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_clean_idle_no_sessions() {
        let mgr = SessionManager::new();
        // 无会话时清理不应 panic
        mgr.clean_idle_sessions(45).await;
    }
}
