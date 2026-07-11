//! WebSocket 原生接入模块
//!
//! 在 TCP 接入之外，同时监听 WebSocket 连接，浏览器可直接连入网关，
//! 无需 ws_proxy.js 中间层。
//!
//! WS 端口默认 7890，通过 GATE_WS_PORT 环境变量配置。
//!
//! 数据流: Browser → WS(:7890) → Gateway → gRPC → LogicServer

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::BytesMut;
use futures_util::{Sink, Stream, SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use crate::cluster::ClusterManager;
use crate::config::AppConfig;
use crate::foundation::GateError;
use crate::grpc_router::RouterManager;
use crate::network::handshake;
use crate::security::SecurityManager;
use crate::session::SessionManager;

/// WebSocket 字节流适配器
///
/// 将 WebSocketStream 适配为 AsyncRead + AsyncWrite，
/// 使握手/ReadLoop/WriteLoop 代码无需修改即可用于 WS 连接。
pub struct WsAdapter {
    /// WS 流 (可读可写)
    stream: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    /// 读取缓冲区 (WS binary message 数据)
    read_buf: BytesMut,
    /// 是否已关闭
    closed: bool,
}

impl WsAdapter {
    pub fn new(stream: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Self {
        Self {
            stream,
            read_buf: BytesMut::new(),
            closed: false,
        }
    }
}

impl AsyncRead for WsAdapter {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();

        // 1. 先从缓冲区读
        if this.read_buf.is_empty() {
            // 2. 从 WS stream 读取下一条消息
            use std::task::Poll;
            match Stream::poll_next(std::pin::Pin::new(&mut this.stream), cx) {
                Poll::Ready(Some(Ok(msg))) => match msg {
                    Message::Binary(data) => {
                        this.read_buf.extend_from_slice(&data);
                    }
                    Message::Text(text) => {
                        this.read_buf.extend_from_slice(text.as_bytes());
                    }
                    Message::Close(_) => {
                        this.closed = true;
                        return Poll::Ready(Ok(()));
                    }
                    Message::Ping(_) | Message::Pong(_) => {
                        // 心跳帧忽略，返回空读
                        return Poll::Ready(Ok(()));
                    }
                    _ => return Poll::Ready(Ok(())),
                },
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        e,
                    )));
                }
                Poll::Ready(None) => {
                    this.closed = true;
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => return Poll::Pending,
            }
        }

        // 3. 从缓冲区拷贝到 ReadBuf
        let n = std::cmp::min(this.read_buf.len(), buf.remaining());
        if n > 0 {
            buf.put_slice(&this.read_buf.split_to(n));
        }
        std::task::Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for WsAdapter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if this.closed {
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "WS closed",
            )));
        }
        // 使用 Sink trait 的 poll_ready / start_send
        use std::task::Poll;
        match Sink::poll_ready(std::pin::Pin::new(&mut this.stream), cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(e)) => {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    e,
                )))
            }
            Poll::Pending => return Poll::Pending,
        }
        match Sink::start_send(std::pin::Pin::new(&mut this.stream), Message::Binary(buf.to_vec().into())) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(e) => Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                e,
            ))),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Sink::poll_flush(std::pin::Pin::new(&mut self.get_mut().stream), cx)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::ConnectionAborted, e))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let _ = Sink::start_send(std::pin::Pin::new(&mut this.stream), Message::Close(None));
        Sink::poll_close(std::pin::Pin::new(&mut this.stream), cx)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::ConnectionAborted, e))
    }
}

impl Unpin for WsAdapter {}

/// WS 接入器主循环
pub async fn run_ws_acceptor(
    session_mgr: Arc<SessionManager>,
    security_mgr: Arc<SecurityManager>,
    router_mgr: Arc<RouterManager>,
    cluster_mgr: Arc<ClusterManager>,
    config: Arc<AppConfig>,
) {
    let ws_addr = format!("{}:{}", config.gate.tcp_bind, config.gate.ws_port);
    let listener = match TcpListener::bind(&ws_addr).await {
        Ok(l) => {
            info!("WebSocket 监听器启动: ws://{}", ws_addr);
            l
        }
        Err(e) => {
            warn!("WebSocket 监听器启动失败: {} — 跳过 WS 支持", e);
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                // IP 黑名单检查
                if security_mgr.is_ip_blocked(&peer_addr.ip()) {
                    debug!("WS 黑名单拒绝: {}", peer_addr);
                    continue;
                }

                let sm = session_mgr.clone();
                let sec = security_mgr.clone();
                let rm = router_mgr.clone();
                let cm = cluster_mgr.clone();
                let cfg = config.clone();

                tokio::spawn(async move {
                    // WS 升级
                    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
                        Ok(ws) => ws,
                        Err(e) => {
                            debug!("WS 升级失败: {} err={}", peer_addr, e);
                            return;
                        }
                    };

                    debug!("WS 连接: {}", peer_addr);

                    // 用 WsAdapter 包装
                    let adapter = WsAdapter::new(ws_stream);

                    // 读写分离
                    let (read_half, write_half) = tokio::io::split(adapter);

                    // 调用通用握手处理
                    if let Err(e) =
                        handshake::handle_connection_split(read_half, write_half, peer_addr, sm, sec, rm, cm, &cfg).await
                    {
                        if !e.to_string().contains("WS closed") && !e.to_string().contains("Connection reset") {
                            debug!("WS 连接结束: {} err={}", peer_addr, e);
                        }
                    }
                });
            }
            Err(e) => {
                warn!("WS accept 错误: {}", e);
            }
        }
    }
}
