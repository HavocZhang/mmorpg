//! # Rust MMO 百万在线网关集群 - 程序入口
//!
//! 职责：信号监听、模块启动编排、优雅启停
//!
//! 架构定位：有状态游戏接入网关，只做四层能力：
//! 1. TCP 长连接接入、会话管理
//! 2. 游戏私有协议编解码、加密、校验、防篡改
//! 3. 客户端 <-> 逻辑服消息路由转发
//! 4. 集群服务发现、跨网关消息同步、安全限流、运维监控

use std::sync::Arc;

use anyhow::Result;
use tracing::info;

use rust_mmo_gate::config::AppConfig;
use rust_mmo_gate::GateContext;
use rust_mmo_gate::{admin, cluster, foundation, grpc_router, network, security, session};

#[tokio::main]
async fn main() -> Result<()> {
    // ── 阶段1：加载配置 ──
    let config = AppConfig::load()?;
    let env = config.app.env.clone();

    // ── 阶段1：初始化日志 ──
    foundation::logger::init(&config.app.log_level, &config.app.log_format);
    info!("========================================");
    info!("  Rust MMO Gate 网关启动中...");
    info!("  环境: {}", env);
    info!("  节点: {} (ID={})", config.gate.node_name, config.gate.node_id);
    info!("  TCP: {}:{}", config.gate.tcp_bind, config.gate.tcp_port);
    info!("  HTTP监控: {}:{}", config.gate.http_bind, config.gate.http_port);
    info!("========================================");

    // ── 全局共享上下文 ──
    let ctx = Arc::new(GateContext {
        config: config.clone(),
    });

    // ── 阶段4：初始化会话管理器 ──
    let session_mgr = Arc::new(session::session_mgr::SessionManager::new());
    info!("✅ 会话管理器初始化完成");

    // ── 阶段8：初始化安全限流模块 ──
    let security_mgr = Arc::new(security::SecurityManager::new(&config));
    info!("✅ 安全限流模块初始化完成");

    // ── 阶段7：初始化Redis集群模块 ──
    let cluster_mgr = Arc::new(cluster::ClusterManager::new(&config).await?);
    if cluster_mgr.is_redis_available() {
        info!("✅ Redis集群模块初始化完成（Redis已连接）");
    } else {
        info!("⚠️ Redis集群模块初始化完成（Redis不可用，降级运行）");
    }

    // ── 阶段6：初始化gRPC路由模块 ──
    let router_mgr = Arc::new(grpc_router::RouterManager::new(&config).await?);
    info!("✅ gRPC路由模块初始化完成");

    // ── 阶段7：启动集群服务（节点注册 + 心跳上报）──
    cluster_mgr.clone().start().await;
    info!("✅ 集群服务已启动（节点注册 + 心跳上报）");

    // ── 阶段4：启动心跳检查定时器（清理僵尸连接）──
    let heartbeat_mgr = session_mgr.clone();
    let heartbeat_timeout = config.session.heartbeat_timeout_secs;
    let heartbeat_interval = config.session.heartbeat_check_interval_secs;
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(heartbeat_interval));
        loop {
            timer.tick().await;
            heartbeat_mgr.clean_idle_sessions(heartbeat_timeout).await;
        }
    });
    info!("✅ 心跳检查定时器已启动（超时={}s 间隔={}s）", heartbeat_timeout, heartbeat_interval);

    // ── 阶段9：启动HTTP监控运维服务 ──
    let admin_handle = tokio::spawn(admin::run_admin_server(
        ctx.clone(),
        session_mgr.clone(),
        security_mgr.clone(),
    ));
    info!("✅ HTTP监控运维服务启动完成");

    // ── 阶段7：启动跨网关 PubSub 订阅器 ──
    if cluster_mgr.is_redis_available() {
        let pubsub_session_mgr = session_mgr.clone();
        let pubsub_node_id = cluster_mgr.node_id;
        let pubsub_redis_url = cluster_mgr.redis_url.clone();
        tokio::spawn(async move {
            let pubsub = cluster::cross_gate_pubsub::CrossGatePubSub::new(
                pubsub_node_id,
                pubsub_redis_url,
            );
            let _ = pubsub
                .subscribe(move |cross_msg| {
                    let priority = match cross_msg.priority {
                        2 => session::session_struct::MsgPriority::High,
                        1 => session::session_struct::MsgPriority::Normal,
                        _ => session::session_struct::MsgPriority::Low,
                    };
                    let msg = session::session_struct::PendingMsg {
                        msg_id: cross_msg.msg_id,
                        payload: cross_msg.payload,
                        priority,
                    };

                    if cross_msg.to_uid == 0 {
                        // 广播给所有本地在线玩家
                        let sids = pubsub_session_mgr.get_all_session_ids();
                        let mut count = 0;
                        for sid in sids {
                            if let Some(session) = pubsub_session_mgr.get_session(sid) {
                                if session.is_online() {
                                    let _ = session.send(msg.clone());
                                    count += 1;
                                }
                            }
                        }
                        if count > 0 {
                            tracing::debug!(
                                "跨网关广播本地分发: msg_id={} count={}",
                                cross_msg.msg_id, count
                            );
                        }
                    } else {
                        // 精准投递给目标玩家
                        if let Some(session) =
                            pubsub_session_mgr.get_session_by_uid(cross_msg.to_uid)
                        {
                            if session.is_online() {
                                let _ = session.send(msg);
                                tracing::debug!(
                                    "跨网关定向本地分发: uid={} msg_id={}",
                                    cross_msg.to_uid, cross_msg.msg_id
                                );
                            }
                        }
                    }
                })
                .await;
        });
        info!("✅ 跨网关 PubSub 订阅器已启动");
    } else {
        info!("⚠️ Redis不可用，跳过 PubSub 订阅器");
    }

    // ── 阶段3+4+5：启动TCP接入服务 ──
    let tcp_handle = tokio::spawn(network::tcp_listener::run_tcp_acceptor(
        ctx.clone(),
        session_mgr.clone(),
        security_mgr.clone(),
        router_mgr.clone(),
        cluster_mgr.clone(),
    ));
    info!("✅ TCP接入服务启动完成，开始接受连接");

    // ── 信号监听：优雅停机 ──
    admin::graceful_shutdown::wait_for_shutdown().await;
    info!("收到停机信号，开始优雅关闭...");

    // 1. 停止接受新连接
    tcp_handle.abort();
    info!("✅ TCP接入服务已停止");

    // 2. 等待存量连接处理（给 5 秒 grace period）
    info!("等待存量连接处理（5秒 grace period）...");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // 3. 清理所有会话
    let session_ids = session_mgr.get_all_session_ids();
    info!("清理 {} 个在线会话...", session_ids.len());
    for sid in session_ids {
        session_mgr.kick_session(sid, "网关停机").await;
    }

    // 4. 注销集群节点
    cluster_mgr.shutdown().await;
    info!("✅ 集群节点已注销");

    // 5. 停止HTTP监控服务
    admin_handle.abort();
    info!("✅ HTTP监控服务已停止");

    info!("✅ 优雅关闭完成，网关已停止");

    Ok(())
}
