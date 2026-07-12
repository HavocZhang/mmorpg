//! UI 面板系统
//!
//! 显示:
//! - 顶部 HUD: 玩家名/等级 + HP/MP/经验可视化条 + 金币/坐标
//! - 居中状态提示: 连接中/等待登录
//! - 死亡覆盖层: "你已死亡 按 R 复活"
//! - 背包面板 (按 I 切换)
//! - 任务面板 (按 Q 切换)
//! - NPC 对话框
//! - 战斗日志 (按 L 切换)
//! - 底部帮助栏

use bevy::prelude::*;

use crate::components::GameFont;
use crate::network::{NetworkCommand, NetworkResource};
use crate::resources::*;

// ============================================================================
// UI 标记组件
// ============================================================================

#[derive(Component)]
pub struct UiRoot;

#[derive(Component)]
pub struct HudText;

#[derive(Component)]
pub struct CenterStatusText;

/// HP 条填充 (UI 层，玩家自己的 HP 条)
#[derive(Component)]
pub struct PlayerHpBarFill;

/// MP 条填充
#[derive(Component)]
pub struct PlayerMpBarFill;

/// 经验条填充
#[derive(Component)]
pub struct PlayerExpBarFill;

#[derive(Component)]
pub struct InventoryPanel;

#[derive(Component)]
pub struct QuestPanel;

#[derive(Component)]
pub struct CombatLogPanel;

#[derive(Component)]
pub struct CombatLogText;

#[derive(Component)]
pub struct DialogPanel;

#[derive(Component)]
pub struct DialogText;

#[derive(Component)]
pub struct DialogOptions;

#[derive(Component)]
pub struct InventoryContent;

#[derive(Component)]
pub struct QuestContent;

// ============================================================================
// UI 初始化
// ============================================================================

/// 创建 UI 根节点和所有面板
pub fn setup_ui(mut commands: Commands, game_font: Res<GameFont>) {
    let font = game_font.font.clone();
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
            // ── 顶部 HUD 状态栏 ──
            parent
                .spawn(NodeBundle {
                    style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Px(70.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    background_color: Color::srgba(0.05, 0.05, 0.1, 0.9).into(),
                    ..default()
                })
                .with_children(|hud| {
                    // 第一行: 玩家名/等级/HP/MP/经验文本
                    hud.spawn((
                        TextBundle::from_section(
                            "Rust MMO - 启动中...",
                            TextStyle {
                                font: font.clone(),
                                font_size: 13.0,
                                color: Color::srgb(0.95, 0.95, 0.95),
                                ..default()
                            },
                        )
                        .with_style(Style {
                            width: Val::Percent(100.0),
                            height: Val::Px(20.0),
                            ..default()
                        }),
                        HudText,
                    ));
                    // 第二行: HP 条 (红绿)
                    hud.spawn(NodeBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            height: Val::Px(12.0),
                            margin: UiRect::top(Val::Px(2.0)),
                            ..default()
                        },
                        background_color: Color::srgb(0.1, 0.05, 0.05).into(),
                        ..default()
                    })
                    .with_children(|hp_bar| {
                        hp_bar.spawn((
                            NodeBundle {
                                style: Style {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                background_color: Color::srgb(0.8, 0.2, 0.2).into(),
                                ..default()
                            },
                            PlayerHpBarFill,
                        ));
                    });
                    // 第三行: MP 条 (蓝)
                    hud.spawn(NodeBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            height: Val::Px(8.0),
                            margin: UiRect::top(Val::Px(1.0)),
                            ..default()
                        },
                        background_color: Color::srgb(0.05, 0.05, 0.1).into(),
                        ..default()
                    })
                    .with_children(|mp_bar| {
                        mp_bar.spawn((
                            NodeBundle {
                                style: Style {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                background_color: Color::srgb(0.2, 0.4, 0.9).into(),
                                ..default()
                            },
                            PlayerMpBarFill,
                        ));
                    });
                    // 第四行: 经验条 (黄)
                    hud.spawn(NodeBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            height: Val::Px(4.0),
                            margin: UiRect::top(Val::Px(1.0)),
                            ..default()
                        },
                        background_color: Color::srgb(0.1, 0.1, 0.05).into(),
                        ..default()
                    })
                    .with_children(|exp_bar| {
                        exp_bar.spawn((
                            NodeBundle {
                                style: Style {
                                    width: Val::Percent(0.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                background_color: Color::srgb(0.9, 0.8, 0.2).into(),
                                ..default()
                            },
                            PlayerExpBarFill,
                        ));
                    });
                });

            // ── 居中状态提示 (连接/登录中) ──
            parent.spawn((
                TextBundle::from_section(
                    "正在连接服务器...",
                    TextStyle {
                        font: font.clone(),
                        font_size: 28.0,
                        color: Color::srgb(1.0, 1.0, 0.4),
                        ..default()
                    },
                )
                .with_style(Style {
                    position_type: PositionType::Absolute,
                    top: Val::Percent(45.0),
                    left: Val::Percent(50.0),
                    margin: UiRect::new(
                        Val::Percent(-50.0),
                        Val::Percent(0.0),
                        Val::Percent(0.0),
                        Val::Percent(0.0),
                    ),
                    ..default()
                }),
                CenterStatusText,
            ));

            // ── 死亡覆盖层 (居中大字) ──
            parent.spawn((
                NodeBundle {
                    style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        position_type: PositionType::Absolute,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        display: Display::None,
                        ..default()
                    },
                    background_color: Color::srgba(0.3, 0.0, 0.0, 0.5).into(),
                    ..default()
                },
                crate::systems::DeathOverlay,
            ))
            .with_children(|overlay| {
                overlay.spawn(TextBundle::from_section(
                    "你已死亡\n按 R 键复活",
                    TextStyle {
                        font: font.clone(),
                        font_size: 48.0,
                        color: Color::srgb(1.0, 0.3, 0.3),
                        ..default()
                    },
                ));
            });

            // ── 背包面板 (左下, 默认隐藏) ──
            parent
                .spawn((
                    NodeBundle {
                        style: Style {
                            width: Val::Px(300.0),
                            height: Val::Px(400.0),
                            position_type: PositionType::Absolute,
                            left: Val::Px(10.0),
                            bottom: Val::Px(40.0),
                            padding: UiRect::all(Val::Px(10.0)),
                            flex_direction: FlexDirection::Column,
                            display: Display::None,
                            ..default()
                        },
                        background_color: Color::srgba(0.05, 0.05, 0.1, 0.95).into(),
                        border_color: Color::srgb(0.4, 0.4, 0.6).into(),
                        ..default()
                    },
                    InventoryPanel,
                ))
                .with_children(|panel| {
                    panel.spawn(TextBundle::from_section(
                        "== 背包 (I) ==",
                        TextStyle {
                            font: font.clone(),
                            font_size: 16.0,
                            color: Color::srgb(0.9, 0.9, 0.4),
                            ..default()
                        },
                    ));
                    panel.spawn((
                        NodeBundle {
                            style: Style {
                                width: Val::Percent(100.0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::clip_y(),
                                ..default()
                            },
                            ..default()
                        },
                        InventoryContent,
                    ));
                });

            // ── 任务面板 (右侧, 默认隐藏) ──
            parent
                .spawn((
                    NodeBundle {
                        style: Style {
                            width: Val::Px(320.0),
                            height: Val::Px(400.0),
                            position_type: PositionType::Absolute,
                            right: Val::Px(10.0),
                            bottom: Val::Px(40.0),
                            padding: UiRect::all(Val::Px(10.0)),
                            flex_direction: FlexDirection::Column,
                            display: Display::None,
                            ..default()
                        },
                        background_color: Color::srgba(0.05, 0.05, 0.1, 0.95).into(),
                        border_color: Color::srgb(0.4, 0.4, 0.6).into(),
                        ..default()
                    },
                    QuestPanel,
                ))
                .with_children(|panel| {
                    panel.spawn(TextBundle::from_section(
                        "== 任务日志 (Q) ==",
                        TextStyle {
                            font: font.clone(),
                            font_size: 16.0,
                            color: Color::srgb(0.9, 0.9, 0.4),
                            ..default()
                        },
                    ));
                    panel.spawn((
                        NodeBundle {
                            style: Style {
                                width: Val::Percent(100.0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::clip_y(),
                                ..default()
                            },
                            ..default()
                        },
                        QuestContent,
                    ));
                });

            // ── 战斗日志 (右上, 默认隐藏) ──
            parent
                .spawn((
                    NodeBundle {
                        style: Style {
                            width: Val::Px(350.0),
                            height: Val::Px(200.0),
                            position_type: PositionType::Absolute,
                            right: Val::Px(10.0),
                            top: Val::Px(80.0),
                            padding: UiRect::all(Val::Px(8.0)),
                            flex_direction: FlexDirection::Column,
                            display: Display::None,
                            ..default()
                        },
                        background_color: Color::srgba(0.05, 0.05, 0.1, 0.9).into(),
                        border_color: Color::srgb(0.3, 0.3, 0.3).into(),
                        ..default()
                    },
                    CombatLogPanel,
                ))
                .with_children(|panel| {
                    panel.spawn(TextBundle::from_section(
                        "== 战斗日志 (L) ==",
                        TextStyle {
                            font: font.clone(),
                            font_size: 12.0,
                            color: Color::srgb(0.7, 0.7, 0.4),
                            ..default()
                        },
                    ));
                    panel.spawn((
                        TextBundle::from_section(
                            "",
                            TextStyle {
                                font: font.clone(),
                                font_size: 12.0,
                                color: Color::srgb(0.8, 0.8, 0.8),
                                ..default()
                            },
                        ),
                        CombatLogText,
                    ));
                });

            // ── NPC 对话框 (底部中央, 默认隐藏) ──
            parent
                .spawn((
                    NodeBundle {
                        style: Style {
                            width: Val::Px(500.0),
                            position_type: PositionType::Absolute,
                            left: Val::Percent(25.0),
                            bottom: Val::Px(50.0),
                            padding: UiRect::all(Val::Px(15.0)),
                            flex_direction: FlexDirection::Column,
                            display: Display::None,
                            ..default()
                        },
                        background_color: Color::srgba(0.08, 0.08, 0.15, 0.97).into(),
                        border_color: Color::srgb(0.6, 0.5, 0.2).into(),
                        ..default()
                    },
                    DialogPanel,
                ))
                .with_children(|panel| {
                    panel.spawn((
                        TextBundle::from_section(
                            "",
                            TextStyle {
                                font: font.clone(),
                                font_size: 14.0,
                                color: Color::srgb(0.9, 0.85, 0.5),
                                ..default()
                            },
                        ),
                        DialogText,
                    ));
                    panel.spawn((
                        NodeBundle {
                            style: Style {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                margin: UiRect::top(Val::Px(10.0)),
                                ..default()
                            },
                            ..default()
                        },
                        DialogOptions,
                    ));
                });

            // ── 底部帮助栏 ──
            parent.spawn((
                NodeBundle {
                    style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Px(28.0),
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(0.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.2, 0.9).into(),
                    ..default()
                },
            ))
            .with_children(|footer| {
                footer.spawn(TextBundle::from_section(
                    "WASD移动 | 左键攻击/NPC/拾取 | I背包 | Q任务 | L战斗日志 | 滚轮缩放 | ESC关闭对话",
                    TextStyle {
                        font: font.clone(),
                        font_size: 12.0,
                        color: Color::srgb(0.7, 0.7, 0.7),
                        ..default()
                    },
                ));
            });
        });
}

// ============================================================================
// UI 更新系统
// ============================================================================

/// 更新 HUD 文本和 HP/MP/经验条
pub fn update_hud_system(
    player: Res<PlayerState>,
    entities: Res<EntityManager>,
    other_players: Res<OtherPlayerManager>,
    conn: Res<ConnectionState>,
    target: Res<TargetEntity>,
    mut text_query: Query<&mut Text, With<HudText>>,
    mut hp_fill: Query<&mut Style, With<PlayerHpBarFill>>,
    mut mp_fill: Query<&mut Style, (With<PlayerMpBarFill>, Without<PlayerHpBarFill>)>,
    mut exp_fill: Query<
        &mut Style,
        (
            With<PlayerExpBarFill>,
            Without<PlayerHpBarFill>,
            Without<PlayerMpBarFill>,
        ),
    >,
) {
    let text = if !conn.connected {
        "Rust MMO - 连接中...".to_string()
    } else if !player.logged_in {
        "Rust MMO - 已连接，等待登录响应...".to_string()
    } else {
        let target_str = if let Some(tid) = target.entity_id {
            if let Some(info) = entities.entities.get(&tid) {
                format!(" | 目标: {} HP:{}/{}", info.name, info.hp, info.max_hp)
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        format!(
            "{} Lv{} | HP:{}/{} | MP:{}/{} | Exp:{}/{} | 金币:{} | ATK:{} DEF:{} | 位置:({:.0},{:.0}) | 附近:{}玩家 {}实体{}",
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
            target_str,
        )
    };

    for mut t in text_query.iter_mut() {
        t.sections[0].value = text.clone();
    }

    // 更新 HP 条宽度
    if player.max_hp > 0 {
        let hp_ratio = (player.hp as f32 / player.max_hp as f32).clamp(0.0, 1.0) * 100.0;
        for mut style in hp_fill.iter_mut() {
            style.width = Val::Percent(hp_ratio);
        }
    }

    // 更新 MP 条宽度
    if player.max_mp > 0 {
        let mp_ratio = (player.mp as f32 / player.max_mp as f32).clamp(0.0, 1.0) * 100.0;
        for mut style in mp_fill.iter_mut() {
            style.width = Val::Percent(mp_ratio);
        }
    }

    // 更新经验条宽度
    if player.max_exp > 0 {
        let exp_ratio = (player.exp as f32 / player.max_exp as f32).clamp(0.0, 1.0) * 100.0;
        for mut style in exp_fill.iter_mut() {
            style.width = Val::Percent(exp_ratio);
        }
    }
}

/// 更新居中状态提示
pub fn update_center_status_system(
    player: Res<PlayerState>,
    conn: Res<ConnectionState>,
    mut text_query: Query<&mut Text, With<CenterStatusText>>,
    mut style_query: Query<&mut Style, With<CenterStatusText>>,
) {
    let (text, visible) = if !conn.connected {
        ("正在连接服务器...".to_string(), true)
    } else if !player.logged_in {
        ("已连接，等待登录响应...".to_string(), true)
    } else {
        (String::new(), false)
    };

    for mut t in text_query.iter_mut() {
        t.sections[0].value = text.clone();
    }
    for mut style in style_query.iter_mut() {
        style.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// 更新面板可见性
pub fn update_panels_system(
    panels: Res<PanelVisibility>,
    mut inv_query: Query<&mut Style, With<InventoryPanel>>,
    mut quest_query: Query<&mut Style, (With<QuestPanel>, Without<InventoryPanel>)>,
    mut log_query: Query<
        &mut Style,
        (
            With<CombatLogPanel>,
            Without<InventoryPanel>,
            Without<QuestPanel>,
        ),
    >,
) {
    for mut style in inv_query.iter_mut() {
        style.display = if panels.inventory {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut style in quest_query.iter_mut() {
        style.display = if panels.quest {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut style in log_query.iter_mut() {
        style.display = if panels.combat_log {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// 更新背包面板内容
pub fn update_inventory_system(
    inventory: Res<Inventory>,
    equipment: Res<Equipment>,
    panels: Res<PanelVisibility>,
    game_font: Res<GameFont>,
    content_parent: Query<Entity, With<InventoryContent>>,
    mut commands: Commands,
) {
    if !panels.inventory {
        return;
    }
    let parent_entity = if let Ok(e) = content_parent.get_single() {
        e
    } else {
        return;
    };

    commands.entity(parent_entity).despawn_descendants();

    let mut text = String::new();
    text.push_str("--- 装备 ---\n");
    let eq = &equipment.data;
    if !eq.weapon.empty {
        text.push_str(&format!(
            "武器: {} +{}\n",
            eq.weapon.name, eq.weapon.enhance_level
        ));
    } else {
        text.push_str("武器: (空)\n");
    }
    if !eq.armor.empty {
        text.push_str(&format!(
            "护甲: {} +{}\n",
            eq.armor.name, eq.armor.enhance_level
        ));
    } else {
        text.push_str("护甲: (空)\n");
    }
    if !eq.accessory.empty {
        text.push_str(&format!(
            "饰品: {} +{}\n\n",
            eq.accessory.name, eq.accessory.enhance_level
        ));
    } else {
        text.push_str("饰品: (空)\n\n");
    }
    text.push_str("--- 背包 ---\n");
    if inventory.items.is_empty() {
        text.push_str("(空)\n");
    } else {
        for item in &inventory.items {
            text.push_str(&format!(
                "{} x{} ({})\n",
                item.name, item.count, item.item_type
            ));
        }
    }

    commands.entity(parent_entity).with_children(|p| {
        p.spawn(TextBundle::from_section(
            text,
            TextStyle {
                font: game_font.font.clone(),
                font_size: 13.0,
                color: Color::srgb(0.85, 0.85, 0.85),
                ..default()
            },
        ));
    });
}

/// 更新任务面板内容
pub fn update_quest_system(
    quest_log: Res<QuestLog>,
    panels: Res<PanelVisibility>,
    game_font: Res<GameFont>,
    content_parent: Query<Entity, With<QuestContent>>,
    mut commands: Commands,
) {
    if !panels.quest {
        return;
    }
    let parent_entity = if let Ok(e) = content_parent.get_single() {
        e
    } else {
        return;
    };

    commands.entity(parent_entity).despawn_descendants();

    let mut text = String::new();
    if quest_log.quests.is_empty() {
        text.push_str("暂无任务\n");
        text.push_str("\n提示: 点击黄色 NPC 对话接任务");
    } else {
        for q in &quest_log.quests {
            let status = if q.completed {
                "[可完成]"
            } else {
                "[进行中]"
            };
            text.push_str(&format!("{} {}\n", status, q.name));
            text.push_str(&format!("  进度: {}/{}\n", q.progress, q.target));
            text.push_str(&format!("  {}\n\n", q.desc));
        }
    }

    commands.entity(parent_entity).with_children(|p| {
        p.spawn(TextBundle::from_section(
            text,
            TextStyle {
                font: game_font.font.clone(),
                font_size: 13.0,
                color: Color::srgb(0.85, 0.85, 0.85),
                ..default()
            },
        ));
    });
}

/// 更新战斗日志
pub fn update_combat_log_system(
    combat_log: Res<CombatLog>,
    panels: Res<PanelVisibility>,
    mut text_query: Query<&mut Text, With<CombatLogText>>,
) {
    if !panels.combat_log {
        return;
    }
    let text = combat_log
        .entries
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    for mut t in text_query.iter_mut() {
        t.sections[0].value = text.clone();
    }
}

/// 更新 NPC 对话框
pub fn update_dialog_system(
    net: Res<NetworkResource>,
    keyboard: Res<ButtonInput<KeyCode>>,
    game_font: Res<GameFont>,
    mut dialog_panel: Query<&mut Style, With<DialogPanel>>,
    mut dialog_text: Query<&mut Text, With<DialogText>>,
    options_parent: Query<Entity, With<DialogOptions>>,
    mut commands: Commands,
    mut dialog_state: ResMut<NpcDialogState>,
) {
    // ESC 关闭对话
    if keyboard.just_pressed(KeyCode::Escape) && dialog_state.dialog.is_some() {
        dialog_state.dialog = None;
    }

    let has_dialog = dialog_state.dialog.is_some();

    for mut style in dialog_panel.iter_mut() {
        style.display = if has_dialog {
            Display::Flex
        } else {
            Display::None
        };
    }

    if !has_dialog {
        return;
    }

    let dialog_info = dialog_state.dialog.clone().unwrap();

    let text = format!("【{}】\n{}", dialog_info.name, dialog_info.dialog);
    for mut t in dialog_text.iter_mut() {
        t.sections[0].value = text.clone();
    }

    // 数字键选择选项
    let options_snapshot: Vec<(usize, DialogAction)> = dialog_info
        .options
        .iter()
        .enumerate()
        .map(|(i, o)| (i, o.action.clone()))
        .collect();
    for (i, action) in &options_snapshot {
        let key = match i {
            0 => KeyCode::Digit1,
            1 => KeyCode::Digit2,
            2 => KeyCode::Digit3,
            3 => KeyCode::Digit4,
            _ => KeyCode::Digit5,
        };
        if keyboard.just_pressed(key) {
            match action {
                DialogAction::AcceptQuest(quest_id) => {
                    net.send(NetworkCommand::Send {
                        msg_id: 1005,
                        payload: crate::codec::encode_accept_quest(*quest_id),
                    });
                    dialog_state.dialog = None;
                }
                DialogAction::CompleteQuest(quest_id) => {
                    net.send(NetworkCommand::Send {
                        msg_id: 1006,
                        payload: crate::codec::encode_complete_quest(*quest_id),
                    });
                    dialog_state.dialog = None;
                }
                DialogAction::OpenShop => {
                    dialog_state.dialog = None;
                }
                DialogAction::Close | DialogAction::None => {
                    dialog_state.dialog = None;
                }
            }
            break;
        }
    }

    // 重建选项 UI
    let parent_entity = if let Ok(e) = options_parent.get_single() {
        e
    } else {
        return;
    };
    commands.entity(parent_entity).despawn_descendants();
    commands.entity(parent_entity).with_children(|p| {
        for (i, opt) in dialog_info.options.iter().enumerate() {
            p.spawn(TextBundle::from_section(
                format!("[{}] {}", i + 1, opt.label),
                TextStyle {
                    font: game_font.font.clone(),
                    font_size: 14.0,
                    color: Color::srgb(0.8, 0.8, 1.0),
                    ..default()
                },
            ));
        }
    });
}
