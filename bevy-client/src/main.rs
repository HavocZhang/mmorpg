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
use bevy::render::view::VisibilitySystems;

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
                        resolution: bevy::window::WindowResolution::new(1280.0, 800.0),
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
        .init_resource::<resources::Inventory>()
        .init_resource::<resources::Equipment>()
        .init_resource::<resources::QuestLog>()
        .init_resource::<resources::DropManager>()
        .init_resource::<resources::NpcDialogState>()
        .init_resource::<resources::TargetEntity>()
        .init_resource::<resources::CombatLog>()
        .init_resource::<resources::PanelVisibility>()
        // 事件注册
        .add_event::<components::DamageEvent>()
        .add_event::<components::ExpGainEvent>()
        .add_event::<components::PlayerDeathEvent>()
        .add_event::<components::PlayerReviveEvent>()
        // 渲染设置: 深蓝灰色背景
        .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.1)))
        // 启动系统 (顺序执行: setup 先加载字体，setup_ui 依赖 GameFont)
        .add_systems(Startup, (setup, systems::setup_world, ui::setup_ui).chain())
        // 每帧更新系统 (分两组避免超过 Bevy 20 个系统元组限制)
        .add_systems(
            Update,
            (
                // 网络事件处理 (最先)
                network::network_event_system,
                // 输入处理
                systems::movement_system,
                systems::mouse_input_system,
                systems::panel_toggle_system,
                // 定时查询实体
                systems::entity_query_timer,
                // 渲染
                systems::render_system,
                systems::update_hp_bar_system,
                // 位置插值 (在 render_system 之后，让 Transform 平滑追随 TargetPosition)
                systems::interpolate_position_system,
                // 伤害/经验飘字
                systems::spawn_damage_text_system,
                systems::spawn_exp_text_system,
                systems::damage_text_system,
                // 相机
                systems::camera_follow_system,
                systems::camera_zoom_system,
                // 死亡处理
                systems::death_system,
                // 连接后拉取配置/实体
                on_connected_system,
            ),
        )
        .add_systems(
            Update,
            (
                // UI 更新
                ui::update_hud_system,
                ui::update_center_status_system,
                ui::update_panels_system,
                ui::update_inventory_system,
                ui::update_quest_system,
                ui::update_combat_log_system,
                ui::update_dialog_system,
            ),
        )
        // 诊断: 在 CheckVisibility 之后检查实体的可见性状态
        .add_systems(
            PostUpdate,
            systems::visibility_diagnostic_system.after(VisibilitySystems::CheckVisibility),
        )
        .run();
}

/// 启动系统: 创建相机并连接服务器
fn setup(
    mut commands: Commands,
    net: Res<network::NetworkResource>,
    asset_server: Res<AssetServer>,
) {
    // 加载中文字体 (simhei.ttf)
    let font = asset_server.load("fonts/simhei.ttf");
    commands.insert_resource(components::GameFont { font: font.clone() });

    // 2D 相机 (初始缩放 1.5x)
    // near=-1000 确保所有 z>0 的 2D 实体不会被近平面剔除
    commands.spawn(Camera2dBundle {
        projection: OrthographicProjection {
            near: -1000.0,
            far: 1000.0,
            scale: 1.5,
            ..default()
        },
        transform: Transform::from_xyz(0.0, 0.0, 999.0),
        ..default()
    });

    // 连接服务器 (TCP 直连网关 7888)
    info!("正在连接服务器 tcp://{}...", "127.0.0.1:7888");
    net.send(network::NetworkCommand::Connect {
        uid: LOGIN_UID,
        token: LOGIN_TOKEN.to_string(),
    });
}

/// 连接成功后拉取配置和实体列表 (只执行一次)
fn on_connected_system(
    net: Res<network::NetworkResource>,
    conn: Res<resources::ConnectionState>,
    player: Res<resources::PlayerState>,
    config: Res<resources::GameConfig>,
    mut sent: Local<bool>,
) {
    if !conn.connected || !player.logged_in || *sent {
        return;
    }
    *sent = true;
    // 拉取配置 (msg_id=101)
    net.send(network::NetworkCommand::Send {
        msg_id: 101,
        payload: crate::codec::encode_query_config(),
    });
    // 查询附近实体 (msg_id=4002)
    net.send(network::NetworkCommand::Send {
        msg_id: 4002,
        payload: crate::codec::encode_query_entities(),
    });
    info!("已发送配置拉取和实体查询请求");
    // config 已在 resource 中，避免未使用警告
    let _ = &config;
}
