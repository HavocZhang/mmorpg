//! 下行分发模块
//!
//! 逻辑服下行包 -> 网关 -> 精准下发对应玩家会话

use std::sync::Arc;

use tracing::{debug, warn};

use crate::foundation::GateError;
use crate::session::session_mgr::SessionManager;
use crate::session::session_struct::{MsgPriority, PendingMsg};

/// 下行分发器
pub struct DownstreamDispatcher {
    session_mgr: Arc<SessionManager>,
}

impl DownstreamDispatcher {
    pub fn new(session_mgr: Arc<SessionManager>) -> Self {
        Self { session_mgr }
    }

    /// 下发消息给指定玩家
    ///
    /// # 参数
    /// - `player_uid`：目标玩家UID
    /// - `msg_id`：消息ID
    /// - `payload`：消息体
    /// - `priority`：消息优先级
    pub fn dispatch(
        &self,
        player_uid: u64,
        msg_id: u16,
        payload: Vec<u8>,
        priority: MsgPriority,
    ) -> Result<(), GateError> {
        let session = self
            .session_mgr
            .get_session_by_uid(player_uid)
            .ok_or_else(|| GateError::SessionNotFound(player_uid))?;

        if !session.is_online() {
            warn!("玩家不在线 uid={}", player_uid);
            return Err(GateError::SessionClosed(session.session_id));
        }

        let msg = PendingMsg {
            msg_id,
            payload,
            priority,
        };

        session.send(msg)?;

        debug!(
            "下行分发: uid={} session_id={} msg_id={}",
            player_uid, session.session_id, msg_id
        );

        Ok(())
    }

    /// 批量下发（广播场景）
    pub fn dispatch_batch(
        &self,
        messages: Vec<(u64, u16, Vec<u8>, MsgPriority)>,
    ) {
        let mut success = 0;
        let mut failed = 0;
        for (uid, msg_id, payload, priority) in messages {
            if self.dispatch(uid, msg_id, payload, priority).is_ok() {
                success += 1;
            } else {
                failed += 1;
            }
        }
        if failed > 0 {
            warn!("批量下发完成: 成功={} 失败={}", success, failed);
        }
    }

    /// 广播消息给所有在线玩家
    pub fn broadcast(&self, msg_id: u16, payload: Vec<u8>, priority: MsgPriority) {
        let session_ids = self.session_mgr.get_all_session_ids();
        let count = session_ids.len();

        for sid in session_ids {
            if let Some(session) = self.session_mgr.get_session(sid) {
                let msg = PendingMsg {
                    msg_id,
                    payload: payload.clone(),
                    priority,
                };
                let _ = session.send(msg);
            }
        }

        debug!("广播完成: {} 个会话", count);
    }
}
