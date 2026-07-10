//! TCP 监听器模块
//!
//! 异步接受TCP连接，为每个连接创建独立异步任务
//! 强制读写分离：ReadHalf / WriteHalf 独立运行互不阻塞

use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::{error, info, warn};

use crate::cluster::ClusterManager;
use crate::config::AppConfig;
use crate::foundation::metric;
use crate::security::SecurityManager;
use crate::session::session_mgr::SessionManager;
use crate::grpc_router::RouterManager;

/// TCP 接受器全局上下文
pub struct TcpAcceptorCtx {
    pub config: AppConfig,
    pub session_mgr: Arc<SessionManager>,
    pub security_mgr: Arc<SecurityManager>,
    pub router_mgr: Arc<RouterManager>,
}

/// 启动 TCP 接受器循环
pub async fn run_tcp_acceptor(
    _ctx: Arc<crate::GateContext>,
    session_mgr: Arc<SessionManager>,
    security_mgr: Arc<SecurityManager>,
    router_mgr: Arc<RouterManager>,
    cluster_mgr: Arc<ClusterManager>,
) {
    let config = &_ctx.config;
    let addr = format!("{}:{}", config.gate.tcp_bind, config.gate.tcp_port);

    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => {
            info!("TCP监听器绑定成功: {}", addr);
            l
        }
        Err(e) => {
            error!("TCP监听器绑定失败 {}: {}", addr, e);
            return;
        }
    };

    // TODO: 实现接受连接循环
    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                // IP 黑名单检查
                if security_mgr.is_ip_blocked(&peer_addr.ip()) {
                    warn!("黑名单IP连接被拒绝: {}", peer_addr);
                    continue;
                }

                // 连接频率限流检查
                if !security_mgr.check_connect_rate(&peer_addr.ip()) {
                    warn!("高频连接限流: {}", peer_addr);
                    continue;
                }

                // 更新连接计数
                metric::metrics().connections.inc();

                // 为每个连接创建独立异步任务
                let session_mgr = session_mgr.clone();
                let security_mgr = security_mgr.clone();
                let router_mgr = router_mgr.clone();
                let cluster_mgr = cluster_mgr.clone();
                let config = config.clone();

                tokio::spawn(async move {
                    if let Err(e) = super::handshake::handle_connection(
                        stream,
                        peer_addr,
                        session_mgr,
                        security_mgr,
                        router_mgr,
                        cluster_mgr,
                        &config,
                    )
                    .await
                    {
                        error!("连接处理错误 {}: {}", peer_addr, e);
                        metric::metrics().connections.dec();
                    }
                });
            }
            Err(e) => {
                error!("accept 连接失败: {}", e);
            }
        }
    }
}
