//! TCP 网络层
//!
//! 架构:
//! - 独立 OS 线程运行 tokio runtime，管理 TCP 连接
//! - 通过 channel 与 Bevy 主线程通信:
//!   * 命令通道 (Bevy → 网络): tokio mpsc unbounded (网络线程可 await)
//!   * 事件通道 (网络 → Bevy): crossbeam unbounded (Bevy 主线程 try_recv)
//! - 连接后分离读写：ReadHalf 在子 task 中读取，WriteHalf 在主循环中发送
//! - Bevy 系统通过 NetworkResource 发送命令、接收事件

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bevy::prelude::*;
use crossbeam_channel::{unbounded as cb_unbounded, Receiver as CbReceiver, Sender as CbSender};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::crypto::Crypto;

/// 网关 TCP 地址
const GATE_ADDR: &str = "127.0.0.1:7888";

/// 网络资源 (Bevy Resource)
///
/// 通过 channel 与网络线程通信:
/// - send_tx: 发送命令到网络线程 (tokio mpsc, send 是同步非阻塞)
/// - recv_rx: 接收网络线程的事件 (crossbeam, try_recv 同步非阻塞)
/// - connected: 连接状态原子标志
#[derive(Resource)]
pub struct NetworkResource {
    pub send_tx: UnboundedSender<NetworkCommand>,
    pub recv_rx: CbReceiver<NetworkEvent>,
    pub connected: Arc<AtomicBool>,
}

impl NetworkResource {
    /// 是否已连接
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// 发送命令 (非阻塞)
    pub fn send(&self, cmd: NetworkCommand) {
        let _ = self.send_tx.send(cmd);
    }
}

/// 上行命令 (Bevy → 网络线程)
pub enum NetworkCommand {
    /// 连接服务器并发送登录包
    Connect { uid: u64, token: String },
    /// 发送游戏消息
    Send { msg_id: u16, payload: Vec<u8> },
    /// 断开连接
    Disconnect,
}

/// 下行事件 (网络线程 → Bevy)
pub enum NetworkEvent {
    /// 连接成功
    Connected,
    /// 断开连接
    Disconnected,
    /// 收到服务器消息
    Message { msg_id: u16, payload: Vec<u8> },
    /// 错误
    Error(String),
}

/// 启动网络线程，返回 NetworkResource
pub fn start_network_thread() -> NetworkResource {
    let (cmd_tx, cmd_rx) = unbounded_channel::<NetworkCommand>();
    let (event_tx, event_rx) = cb_unbounded::<NetworkEvent>();
    let connected = Arc::new(AtomicBool::new(false));
    let connected_clone = connected.clone();

    std::thread::Builder::new()
        .name("network-thread".to_string())
        .spawn(move || {
            println!("[NET] 网络线程已启动");
            let panic_tx = event_tx.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("创建tokio runtime失败");
                println!("[NET] tokio runtime 已创建");

                rt.block_on(network_main_loop(cmd_rx, event_tx, connected_clone));
            }));

            match result {
                Ok(_) => println!("[NET] 网络线程正常退出"),
                Err(e) => {
                    let msg = if let Some(s) = e.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = e.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "未知 panic".to_string()
                    };
                    println!("[NET] 网络线程 panic: {}", msg);
                    let _ = panic_tx.send(NetworkEvent::Error(format!("网络线程 panic: {}", msg)));
                }
            }
        })
        .expect("启动网络线程失败");

    NetworkResource {
        send_tx: cmd_tx,
        recv_rx: event_rx,
        connected,
    }
}

/// 网络主循环
async fn network_main_loop(
    mut cmd_rx: UnboundedReceiver<NetworkCommand>,
    event_tx: CbSender<NetworkEvent>,
    connected: Arc<AtomicBool>,
) {
    let crypto = Crypto::new();

    loop {
        // 无连接时，等待 Connect 命令
        let (uid, token) = loop {
            match cmd_rx.recv().await {
                Some(NetworkCommand::Connect { uid, token }) => break (uid, token),
                Some(NetworkCommand::Send { .. }) | Some(NetworkCommand::Disconnect) => {
                    // 未连接时忽略
                }
                None => return,
            }
        };

        // 建立 TCP 连接
        println!("[NET] 正在连接 {}...", GATE_ADDR);
        match TcpStream::connect(GATE_ADDR).await {
            Ok(stream) => {
                stream.set_nodelay(true).ok();
                println!("[NET] TCP 连接成功");

                // 发送握手包 (msg_id=1, JSON HandshakePayload)
                let login_payload = crate::codec::encode_login_json(uid, &token);
                let packet = crypto.pack(1, &login_payload);

                // 分离读写
                let (read_half, mut write_half) = stream.into_split();

                // 先发送握手包
                if let Err(e) = write_half.write_all(&packet).await {
                    let _ = event_tx.send(NetworkEvent::Error(format!(
                        "发送握手包失败: {}", e
                    )));
                    continue;
                }

                connected.store(true, Ordering::Relaxed);
                let _ = event_tx.send(NetworkEvent::Connected);

                // spawn 读取 task: read_exact 不会被 select! 取消
                // 通过 tokio mpsc 把消息转发给主循环
                let (read_tx, mut read_rx) = unbounded_channel::<NetworkEvent>();
                let read_crypto = Crypto::new();
                let read_handle = tokio::spawn(async move {
                    let mut stream = read_half;
                    loop {
                        let mut header = [0u8; 16];
                        match stream.read_exact(&mut header).await {
                            Ok(_) => {
                                let body_len = u16::from_be_bytes([
                                    header[6], header[7],
                                ]) as usize;
                                let mut body = vec![0u8; body_len];
                                match stream.read_exact(&mut body).await {
                                    Ok(_) => {
                                        let mut full = header.to_vec();
                                        full.extend_from_slice(&body);
                                        if let Some((msg_id, payload)) =
                                            read_crypto.unpack(&full)
                                        {
                                            let _ = read_tx.send(
                                                NetworkEvent::Message {
                                                    msg_id,
                                                    payload,
                                                },
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        let _ = read_tx.send(NetworkEvent::Error(
                                            format!("读取包体失败: {}", e),
                                        ));
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                                    let _ = read_tx.send(NetworkEvent::Disconnected);
                                } else {
                                    let _ = read_tx.send(NetworkEvent::Error(
                                        format!("读取包头失败: {}", e),
                                    ));
                                }
                                break;
                            }
                        }
                    }
                });

                // 主循环: select! 同时处理命令和服务器消息
                loop {
                    tokio::select! {
                        // 服务器消息 (从读取 task 转发)
                        event = read_rx.recv() => {
                            match event {
                                Some(event) => {
                                    match &event {
                                        NetworkEvent::Disconnected
                                        | NetworkEvent::Error(_) => {
                                            connected.store(false, Ordering::Relaxed);
                                        }
                                        _ => {}
                                    }
                                    let _ = event_tx.send(event);
                                }
                                None => {
                                    // 读取 task 结束 (read_tx dropped)
                                    break;
                                }
                            }
                        }
                        // 命令 (Bevy → 网络)
                        cmd = cmd_rx.recv() => {
                            match cmd {
                                Some(NetworkCommand::Send { msg_id, payload }) => {
                                    let packet = crypto.pack(msg_id, &payload);
                                    if let Err(e) = write_half.write_all(&packet).await {
                                        let _ = event_tx.send(NetworkEvent::Error(
                                            format!("发送失败: {}", e),
                                        ));
                                        break;
                                    }
                                }
                                Some(NetworkCommand::Disconnect) => {
                                    let _ = write_half.shutdown().await;
                                    connected.store(false, Ordering::Relaxed);
                                    let _ = event_tx.send(NetworkEvent::Disconnected);
                                    break;
                                }
                                Some(NetworkCommand::Connect { .. }) => {
                                    // 已连接，忽略
                                }
                                None => {
                                    // Bevy 主线程退出 (send_tx dropped)，关闭连接
                                    let _ = write_half.shutdown().await;
                                    return;
                                }
                            }
                        }
                    }
                }

                // 等待读取 task 结束
                let _ = read_handle.await;
                println!("[NET] 连接已关闭");
            }
            Err(e) => {
                let _ = event_tx.send(NetworkEvent::Error(format!(
                    "连接失败: {}", e
                )));
            }
        }
    }
}

/// Bevy 系统: 处理网络事件
///
/// 从网络线程的事件 channel 中取出消息，分发到对应的处理逻辑
pub fn network_event_system(
    net: Res<NetworkResource>,
    mut player_state: ResMut<crate::resources::PlayerState>,
    mut entities: ResMut<crate::resources::EntityManager>,
    mut other_players: ResMut<crate::resources::OtherPlayerManager>,
    mut conn_state: ResMut<crate::resources::ConnectionState>,
    mut game_config: ResMut<crate::resources::GameConfig>,
    mut windows: Query<&mut bevy::window::Window>,
) {
    while let Ok(event) = net.recv_rx.try_recv() {
        match event {
            NetworkEvent::Connected => {
                conn_state.connected = true;
                conn_state.connecting = false;
                println!("[NET] 已连接到服务器");
                if let Ok(mut window) = windows.get_single_mut() {
                    window.title = "Rust MMO - 已连接".to_string();
                }
            }
            NetworkEvent::Disconnected => {
                conn_state.connected = false;
                conn_state.connecting = false;
                player_state.logged_in = false;
                println!("[NET] 断开连接");
                if let Ok(mut window) = windows.get_single_mut() {
                    window.title = "Rust MMO - 已断开".to_string();
                }
            }
            NetworkEvent::Message { msg_id, payload } => {
                println!("[NET] 收到消息 msg_id={} len={}", msg_id, payload.len());
                crate::systems::handle_server_message(
                    msg_id,
                    &payload,
                    &mut player_state,
                    &mut entities,
                    &mut other_players,
                    &mut game_config,
                );
            }
            NetworkEvent::Error(e) => {
                println!("[NET] 网络错误: {}", e);
            }
        }
    }
}
