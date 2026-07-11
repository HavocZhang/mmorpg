//! HTTP 监控运维 + 优雅启停模块
//!
//! 阶段9核心：状态查询、踢人接口、指标上报、信号捕获、优雅关闭
//! 使用 actix-web 4 提供以下 REST API：
//! - GET  /health           - 健康检查
//! - GET  /metrics          - Prometheus 指标
//! - GET  /sessions         - 在线会话列表
//! - POST /kick/{uid}       - 踢人
//! - GET  /blacklist        - 黑名单列表
//! - POST /blacklist/{ip}   - 添加黑名单
//! - DELETE /blacklist/{ip} - 移除黑名单

pub mod graceful_shutdown;
pub mod monitor_api;
pub mod prom_export;

use std::sync::Arc;
use std::time::Instant;

use actix_web::{web, App, HttpServer, HttpResponse,Responder};
use serde::Serialize;
use tracing::info;

use crate::security::SecurityManager;
use crate::session::session_mgr::SessionManager;

/// HTTP 服务共享状态
pub struct AppState {
    pub session_mgr: Arc<SessionManager>,
    pub security_mgr: Arc<SecurityManager>,
    pub started_at: Instant,
    pub node_name: String,
    pub node_id: u64,
}

/// 健康检查响应
#[derive(Serialize)]
struct HealthResponse {
    status: String,
    online_count: usize,
    uptime_secs: u64,
    node_name: String,
    node_id: u64,
}

/// 踢人请求响应
#[derive(Serialize)]
struct KickResponse {
    success: bool,
    message: String,
}

/// 黑名单操作响应
#[derive(Serialize)]
struct BlacklistResponse {
    success: bool,
    message: String,
}

/// 启动 HTTP 监控运维服务
pub async fn run_admin_server(
    ctx: Arc<crate::GateContext>,
    session_mgr: Arc<SessionManager>,
    security_mgr: Arc<SecurityManager>,
) {
    let config = ctx.config.clone();
    let addr = format!("{}:{}", config.gate.http_bind, config.gate.http_port);

    info!("HTTP监控服务启动: {}", addr);

    let state = web::Data::new(AppState {
        session_mgr,
        security_mgr,
        started_at: Instant::now(),
        node_name: config.gate.node_name.clone(),
        node_id: config.gate.node_id,
    });

    let server = match HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/health", web::get().to(health_handler))
            .route("/metrics", web::get().to(metrics_handler))
            .route("/sessions", web::get().to(sessions_handler))
            .route("/kick/{uid}", web::post().to(kick_handler))
            .route("/blacklist", web::get().to(list_blacklist_handler))
            .route("/blacklist/{ip}", web::post().to(add_blacklist_handler))
            .route("/blacklist/{ip}", web::delete().to(remove_blacklist_handler))
            .route("/merge_stats", web::get().to(merge_stats_handler))
    })
    .bind(&addr)
    {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("HTTP监控服务绑定失败 {}: {}", addr, e);
            return;
        }
    };

    if let Err(e) = server.run().await {
        tracing::error!("HTTP监控服务异常: {}", e);
    }
}

/// GET /health - 健康检查
async fn health_handler(state: web::Data<AppState>) -> impl Responder {
    let resp = HealthResponse {
        status: "ok".to_string(),
        online_count: state.session_mgr.online_count(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        node_name: state.node_name.clone(),
        node_id: state.node_id,
    };
    HttpResponse::Ok().json(resp)
}

/// GET /metrics - Prometheus 指标
async fn metrics_handler() -> impl Responder {
    let output = prom_export::export();
    HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4")
        .body(output)
}

/// GET /sessions - 在线会话列表
async fn sessions_handler(state: web::Data<AppState>) -> impl Responder {
    let api = monitor_api::MonitorApi::new(state.session_mgr.clone());
    let sessions = api.list_sessions();
    HttpResponse::Ok().json(sessions)
}

/// POST /kick/{uid} - 踢人
async fn kick_handler(
    state: web::Data<AppState>,
    path: web::Path<u64>,
) -> impl Responder {
    let uid = path.into_inner();
    match state.session_mgr.get_session_by_uid(uid) {
        Some(session) => {
            let session_id = session.session_id;
            let reason = "运维踢人";
            state.session_mgr.kick_session(session_id, reason).await;
            HttpResponse::Ok().json(KickResponse {
                success: true,
                message: format!("已踢出玩家 uid={} session_id={}", uid, session_id),
            })
        }
        None => {
            HttpResponse::Ok().json(KickResponse {
                success: false,
                message: format!("玩家不在线 uid={}", uid),
            })
        }
    }
}

/// GET /blacklist - 黑名单列表
async fn list_blacklist_handler(state: web::Data<AppState>) -> impl Responder {
    let ips = state.security_mgr.ip_blacklist.list_all();
    HttpResponse::Ok().json(serde_json::json!({
        "count": ips.len(),
        "ips": ips,
    }))
}

/// POST /blacklist/{ip} - 添加黑名单
async fn add_blacklist_handler(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    let ip_str = path.into_inner();
    match ip_str.parse::<std::net::IpAddr>() {
        Ok(ip) => {
            state.security_mgr.ip_blacklist.add(&ip);
            info!("运维添加IP黑名单: {}", ip);
            HttpResponse::Ok().json(BlacklistResponse {
                success: true,
                message: format!("已添加黑名单: {}", ip),
            })
        }
        Err(_) => {
            HttpResponse::BadRequest().json(BlacklistResponse {
                success: false,
                message: format!("无效的IP地址: {}", ip_str),
            })
        }
    }
}

/// DELETE /blacklist/{ip} - 移除黑名单
async fn remove_blacklist_handler(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    let ip_str = path.into_inner();
    match ip_str.parse::<std::net::IpAddr>() {
        Ok(ip) => {
            state.security_mgr.ip_blacklist.remove(&ip);
            info!("运维移除IP黑名单: {}", ip);
            HttpResponse::Ok().json(BlacklistResponse {
                success: true,
                message: format!("已移除黑名单: {}", ip),
            })
        }
        Err(_) => {
            HttpResponse::BadRequest().json(BlacklistResponse {
                success: false,
                message: format!("无效的IP地址: {}", ip_str),
            })
        }
    }
}

/// GET /merge_stats - 合包压缩率统计
#[allow(clippy::manual_checked_ops)]
async fn merge_stats_handler() -> impl Responder {
    let snap = crate::io_engine::packet_merge::merge_stats_with_recent();
    HttpResponse::Ok().json(serde_json::json!({
        "total_packets_merged": snap.total_packets,
        "total_flush_calls": snap.total_flushes,
        "total_bytes_sent": snap.total_bytes,
        "compression_rate_pct": format!("{:.2}", snap.cumulative_rate),
        "avg_packets_per_flush": if snap.total_flushes > 0 { snap.total_packets / snap.total_flushes } else { 0 },
        "recent_packets": snap.recent_packets,
        "recent_flushes": snap.recent_flushes,
        "recent_compression_rate_pct": format!("{:.2}", snap.recent_rate),
        "recent_avg_packets_per_flush": if snap.recent_flushes > 0 { snap.recent_packets / snap.recent_flushes } else { 0 },
        "recent_bytes_per_sec": snap.bytes_per_sec,
        "recent_window_secs": snap.elapsed_secs,
        "gate_target": ">=70%",
        "gate_pass": snap.cumulative_rate >= 70.0,
        "recent_gate_pass": snap.recent_rate >= 70.0,
    }))
}
