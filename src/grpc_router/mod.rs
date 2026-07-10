//! gRPC 内网路由模块
//!
//! 阶段6核心：连接池、上行路由、下行分发、批量消息处理、断连降级

pub mod conn_pool;
pub mod downstream;
pub mod proto;
pub mod upstream;

use std::sync::Arc;

use crate::config::AppConfig;
use crate::foundation::GateError;

/// 路由管理器
pub struct RouterManager {
    pub conn_pool: std::sync::Arc<conn_pool::GrpcConnPool>,
}

impl RouterManager {
    pub async fn new(config: &AppConfig) -> Result<Self, GateError> {
        let endpoints: Vec<String> = config
            .grpc
            .logic_endpoints
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let conn_pool = std::sync::Arc::new(conn_pool::GrpcConnPool::new(endpoints));
        Ok(Self { conn_pool })
    }
}
