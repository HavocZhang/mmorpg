//! 监控 API 模块
//!
//! HTTP 接口定义：
//! - GET  /health           - 健康检查
//! - GET  /sessions         - 在线会话列表
//! - POST /kick/{uid}       - 踢人
//! - GET  /blacklist        - 黑名单列表
//! - POST /blacklist/{ip}   - 添加黑名单
//! - DELETE /blacklist/{ip} - 移除黑名单

use std::sync::Arc;

use serde::Serialize;

use crate::session::session_mgr::SessionManager;

/// 健康检查响应
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub online_count: usize,
    pub uptime_secs: u64,
}

/// 会话信息
#[derive(Serialize)]
pub struct SessionInfo {
    pub session_id: u64,
    pub player_uid: u64,
    pub peer_addr: String,
    pub state: String,
}

/// 监控 API 处理器
pub struct MonitorApi {
    session_mgr: Arc<SessionManager>,
}

impl MonitorApi {
    pub fn new(session_mgr: Arc<SessionManager>) -> Self {
        Self { session_mgr }
    }

    /// 健康检查
    pub fn health(&self) -> HealthResponse {
        HealthResponse {
            status: "ok".to_string(),
            online_count: self.session_mgr.online_count(),
            uptime_secs: 0,
        }
    }

    /// 获取在线会话列表
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        self.session_mgr
            .get_all_session_ids()
            .into_iter()
            .filter_map(|sid| {
                self.session_mgr.get_session(sid).map(|s| SessionInfo {
                    session_id: s.session_id,
                    player_uid: s.player_uid(),
                    peer_addr: s.peer_addr.to_string(),
                    state: format!("{:?}", s.state()),
                })
            })
            .collect()
    }
}
