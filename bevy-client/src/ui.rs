//! UI 面板系统
//!
//! 显示 HP/MP/经验条、等级、金币等信息

use bevy::prelude::*;

use crate::resources::*;

/// UI 根节点标记
#[derive(Component)]
pub struct UiRoot;

/// HUD 文本标记 (用于查找并更新)
#[derive(Component)]
pub struct HudText;

/// 状态栏文本标记
#[derive(Component)]
pub struct StatusBarText;

/// 创建 UI 根节点和 HUD 元素
pub fn setup_ui(mut commands: Commands) {
    // UI 根节点 (全屏覆盖层)
    commands
        .spawn((
            NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                background_color: Color::NONE.into(),
                ..default()
            },
            UiRoot,
        ))
        .with_children(|parent| {
            // 顶部状态栏
            parent.spawn((
                NodeBundle {
                    style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Px(40.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.2, 0.85).into(),
                    ..default()
                },
                StatusBarText,
            ))
            .with_children(|bar| {
                // 状态文本
                bar.spawn((
                    TextBundle::from_section(
                        "未连接",
                        TextStyle {
                            font_size: 16.0,
                            color: Color::srgb(0.9, 0.9, 0.9),
                            ..default()
                        },
                    )
                    .with_style(Style {
                        margin: UiRect::left(Val::Px(10.0)),
                        ..default()
                    }),
                    HudText,
                ));
            });

            // 底部帮助信息
            parent.spawn((
                NodeBundle {
                    style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Px(30.0),
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(0.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.2, 0.85).into(),
                    ..default()
                },
            ))
            .with_children(|footer| {
                footer.spawn(TextBundle::from_section(
                    "WASD 移动 | Bevy Client v0.1",
                    TextStyle {
                        font_size: 12.0,
                        color: Color::srgb(0.6, 0.6, 0.6),
                        ..default()
                    },
                ));
            });
        });
}

/// 更新 HUD 文本
pub fn update_ui_system(
    player: Res<PlayerState>,
    entities: Res<EntityManager>,
    other_players: Res<OtherPlayerManager>,
    conn: Res<ConnectionState>,
    mut text_query: Query<&mut Text, With<HudText>>,
) {
    let text = if !conn.connected {
        "连接中...".to_string()
    } else if !player.logged_in {
        "已连接，等待登录响应...".to_string()
    } else {
        format!(
            "{} | Lv{} | HP:{}/{} | MP:{}/{} | Exp:{}/{} | 金币:{} | ATK:{} DEF:{} | 位置:({:.0},{:.0}) | 附近:{}玩家 {}实体",
            player.name,
            player.level,
            player.hp,
            player.max_hp,
            player.mp,
            player.max_mp,
            player.exp,
            player.max_exp,
            player.gold,
            player.atk,
            player.def,
            player.x,
            player.y,
            other_players.players.len(),
            entities.entities.len(),
        )
    };

    for mut t in text_query.iter_mut() {
        t.sections[0].value = text.clone();
    }
}
