//! Bevy 客户端入口
//!
//! 启动 Bevy 应用，初始化网络线程，连接到 MMO 网关。

mod codec;
mod components;
mod crypto;
mod network;
mod resources;
mod systems;
mod ui;

use bevy::log::LogPlugin;
use bevy::prelude::*;

/// 登录用的测试 UID
const LOGIN_UID: u64 = 12345;
/// 登录用的测试 Token
const LOGIN_TOKEN: &str = "tok_abcdefgh";

fn main() {
    // 先启动网络线程
    let network = network::start_network_thread();

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(LogPlugin {
                    level: bevy::log::Level::INFO,
                    filter: "bevy_client=info,wgpu=error,naga=warn".to_string(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Rust MMO - Bevy Client".to_string(),
                        resolution: bevy::window::WindowResolution::new(1024.0, 768.0),
                        ..default()
                    }),
                    ..default()
                }),
        )
        // 网络资源
        .insert_resource(network)
        // 游戏状态资源
        .init_resource::<resources::PlayerState>()
        .init_resource::<resources::EntityManager>()
        .init_resource::<resources::OtherPlayerManager>()
        .init_resource::<resources::GameConfig>()
        .init_resource::<resources::InputState>()
        .init_resource::<resources::ConnectionState>()
        .init_resource::<resources::UiTextCache>()
        // 渲染设置
        .insert_resource(ClearColor(Color::srgb(0.03, 0.03, 0.08)))
        // 启动系统
        .add_systems(Startup, (setup, ui::setup_ui))
        // 每帧更新系统
        .add_systems(
            Update,
            (
                network::network_event_system,
                systems::movement_system,
                systems::render_system,
                systems::camera_follow_system,
                ui::update_ui_system,
            ),
        )
        .run();
}

/// 启动系统: 创建相机并连接服务器
fn setup(mut commands: Commands, net: Res<network::NetworkResource>) {
    // 2D 相机
    commands.spawn(Camera2dBundle::default());

    // 创建一个简单的背景网格 (可选, 用一个大平面表示游戏区域)
    // 暂不添加, 让 ClearColor 处理背景

    // 连接服务器 (TCP 直连网关 7888)
    info!("正在连接服务器 tcp://{}...", "127.0.0.1:7888");
    net.send(network::NetworkCommand::Connect {
        uid: LOGIN_UID,
        token: LOGIN_TOKEN.to_string(),
    });

    // 连接后会在 network_event_system 中收到 Connected 事件
    // 登录包会在网络线程中自动发送
}
