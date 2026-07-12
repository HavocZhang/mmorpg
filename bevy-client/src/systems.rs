//! ECS 系统定义
//!
//! 系统是 Bevy ECS 中处理实体和资源的逻辑函数。
//!
//! 包含:
//! - handle_server_message: 服务器下行消息分发 (被 network_event_system 调用)
//! - movement_system: WASD 键盘移动
//! - mouse_input_system: 鼠标点击攻击/NPC交互/拾取
//! - panel_toggle_system: I/Q/L 键切换面板
//! - render_system: 同步游戏实体到 Bevy ECS (Sprite + HP条 + 名称)
//! - camera_follow_system: 相机跟随玩家
//! - setup_world: 创建世界网格背景

use bevy::prelude::*;

use crate::components::*;
use crate::network::{NetworkCommand, NetworkResource};
use crate::resources::*;

// ============================================================================
// 世界初始化
// ============================================================================

/// 世界网格标记
#[derive(Component)]
pub struct WorldGrid;

/// 创建世界背景: 网格 + 边界标记
pub fn setup_world(mut commands: Commands) {
    // ── 网格背景 ──
    // 生成 40x40 的网格线，每格 50px，覆盖 -1000 ~ +1000
    const GRID_SIZE: i32 = 40;
    const CELL: f32 = 50.0;
    const HALF: f32 = (GRID_SIZE as f32) * CELL / 2.0;

    // 竖线
    for i in 0..=GRID_SIZE {
        let x = -HALF + i as f32 * CELL;
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::srgba(0.15, 0.15, 0.25, 0.5),
                    custom_size: Some(Vec2::new(1.0, GRID_SIZE as f32 * CELL)),
                    ..default()
                },
                transform: Transform::from_xyz(x, 0.0, -1.0),
                ..default()
            },
            WorldGrid,
        ));
    }
    // 横线
    for i in 0..=GRID_SIZE {
        let y = -HALF + i as f32 * CELL;
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::srgba(0.15, 0.15, 0.25, 0.5),
                    custom_size: Some(Vec2::new(GRID_SIZE as f32 * CELL, 1.0)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, y, -1.0),
                ..default()
            },
            WorldGrid,
        ));
    }

    // ── 原点标记 (红十字) ──
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::srgb(0.5, 0.2, 0.2),
                custom_size: Some(Vec2::new(40.0, 2.0)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 0.0),
            ..default()
        },
        WorldGrid,
    ));
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::srgb(0.5, 0.2, 0.2),
                custom_size: Some(Vec2::new(2.0, 40.0)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 0.0),
            ..default()
        },
        WorldGrid,
    ));

    // ── 世界边界框 ──
    const WORLD_HALF: f32 = 800.0;
    let border_color = Color::srgb(0.4, 0.3, 0.2);
    // 上边
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: border_color,
                custom_size: Some(Vec2::new(WORLD_HALF * 2.0, 4.0)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, WORLD_HALF, 0.0),
            ..default()
        },
        WorldGrid,
    ));
    // 下边
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: border_color,
                custom_size: Some(Vec2::new(WORLD_HALF * 2.0, 4.0)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, -WORLD_HALF, 0.0),
            ..default()
        },
        WorldGrid,
    ));
    // 左边
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: border_color,
                custom_size: Some(Vec2::new(4.0, WORLD_HALF * 2.0)),
                ..default()
            },
            transform: Transform::from_xyz(-WORLD_HALF, 0.0, 0.0),
            ..default()
        },
        WorldGrid,
    ));
    // 右边
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: border_color,
                custom_size: Some(Vec2::new(4.0, WORLD_HALF * 2.0)),
                ..default()
            },
            transform: Transform::from_xyz(WORLD_HALF, 0.0, 0.0),
            ..default()
        },
        WorldGrid,
    ));

    info!("世界网格已生成 ({}x{}, 边界 {}x{})", GRID_SIZE, GRID_SIZE, WORLD_HALF * 2.0, WORLD_HALF * 2.0);
}

// ============================================================================
// 服务器消息处理
// ============================================================================

/// 处理服务器下行消息 (由 network_event_system 调用)
pub fn handle_server_message(
    msg_id: u16,
    payload: &[u8],
    player: &mut PlayerState,
    entities: &mut EntityManager,
    other_players: &mut OtherPlayerManager,
    inventory: &mut Inventory,
    equipment: &mut Equipment,
    quest_log: &mut QuestLog,
    drops: &mut DropManager,
    dialog_state: &mut NpcDialogState,
    combat_log: &mut CombatLog,
    game_config: &mut GameConfig,
) {
    match msg_id {
        // 5001: 玩家属性 (proto)
        5001 => {
            if let Some(stats) = crate::codec::decode_player_stats(payload) {
                player.uid = stats.uid;
                player.name = stats.name;
                player.hp = stats.hp;
                player.max_hp = stats.max_hp;
                player.mp = stats.mp;
                player.max_mp = stats.max_mp;
                player.level = stats.level;
                player.exp = stats.exp;
                player.max_exp = stats.max_exp;
                player.gold = stats.gold;
                player.atk = stats.atk;
                player.def = stats.def;
                player.x = stats.x;
                player.y = stats.y;
                if !player.logged_in {
                    player.logged_in = true;
                    info!(
                        "登录成功: {} (UID={}, Lv{}, HP={}/{})",
                        player.name, player.uid, player.level, player.hp, player.max_hp
                    );
                }
            }
        }

        // 5002: 经验/MP 更新
        5002 => {
            if let Some(update) = crate::codec::decode_exp_update(payload) {
                if update.is_mp_update {
                    player.mp = update.mp;
                    if update.max_mp > 0 {
                        player.max_mp = update.max_mp;
                    }
                } else {
                    player.exp = update.exp;
                    if update.max_exp > 0 {
                        player.max_exp = update.max_exp;
                    }
                    if update.level > 0 {
                        player.level = update.level;
                    }
                    if update.gained > 0 {
                        combat_log.push(format!("获得 {} 经验", update.gained));
                    }
                }
            }
        }

        // 5003: 背包更新
        5003 => {
            if let Some(update) = crate::codec::decode_inventory_update(payload) {
                inventory.items = update
                    .items
                    .into_iter()
                    .map(|i| InventoryItem {
                        item_id: i.item_id,
                        count: i.count,
                        name: i.name,
                        item_type: i.item_type,
                        icon: i.icon,
                    })
                    .collect();
            }
        }

        // 5004: 装备更新
        5004 => {
            if let Some(update) = crate::codec::decode_equipment_update(payload) {
                let conv = |s: Option<rust_mmo_gate::game_proto::EquipmentSlot>| -> EquipmentSlot {
                    match s {
                        Some(s) => EquipmentSlot {
                            item_id: s.item_id,
                            name: s.name,
                            icon: s.icon,
                            enhance_level: s.enhance_level,
                            empty: s.empty,
                        },
                        None => EquipmentSlot::default(),
                    }
                };
                equipment.data.weapon = conv(update.weapon);
                equipment.data.armor = conv(update.armor);
                equipment.data.accessory = conv(update.accessory);
            }
        }

        // 5005: 任务更新
        5005 => {
            if let Some(update) = crate::codec::decode_quest_update(payload) {
                quest_log.quests = update
                    .quests
                    .into_iter()
                    .map(|q| QuestEntry {
                        quest_id: q.quest_id,
                        name: q.name,
                        progress: q.progress,
                        target: q.target,
                        desc: q.desc,
                        completed: q.completed,
                    })
                    .collect();
            }
        }

        // 5006: NPC 对话
        5006 => {
            if let Some(dialog) = crate::codec::decode_npc_dialog(payload) {
                let options = parse_dialog_options(&dialog.options_json, dialog.npc_id);
                dialog_state.dialog = Some(NpcDialogInfo {
                    npc_id: dialog.npc_id,
                    name: dialog.name,
                    dialog: dialog.dialog,
                    options,
                });
            }
        }

        // 6001: 战斗结果
        6001 => {
            if let Some(result) = crate::codec::decode_combat_result(payload) {
                if !result.error.is_empty() {
                    combat_log.push(format!("战斗错误: {}", result.error));
                } else if result.miss {
                    combat_log.push(format!("攻击 {} 未命中!", result.target_name));
                } else if result.swing {
                    combat_log.push(format!(
                        "普攻 {} 造成 {} 伤害 (剩余HP: {})",
                        result.target_name, result.damage, result.target_hp
                    ));
                } else {
                    combat_log.push(format!(
                        "技能攻击 {} 造成 {} 伤害 (暴击: {})",
                        result.target_name, result.damage, result.crit
                    ));
                }
            }
        }

        // 6002: 实体状态更新
        6002 => {
            if let Some(state) = crate::codec::decode_entity_state(payload) {
                if let Some(info) = entities.entities.get_mut(&state.entity_id) {
                    info.hp = state.hp;
                    info.max_hp = state.max_hp;
                    info.x = state.x;
                    info.y = state.y;
                }
            }
        }

        // 6003: 实体死亡
        6003 => {
            if let Some(death) = crate::codec::decode_entity_death(payload) {
                combat_log.push(format!("击杀 {} (+{}经验)", death.mob_name, death.exp));
                entities.entities.remove(&death.entity_id);
                for drop in &death.drops {
                    drops.drops.insert(
                        drop.drop_id,
                        DropItem {
                            drop_id: drop.drop_id,
                            item_id: drop.item_id,
                            count: drop.count,
                            x: drop.x,
                            y: drop.y,
                        },
                    );
                }
            }
        }

        // 8001: 玩家位置更新 (自己或其他玩家)
        8001 => {
            if let Some(pos) = crate::codec::decode_player_position(payload) {
                if pos.uid == player.uid {
                    player.x = pos.x;
                    player.y = pos.y;
                } else if let Some(p) = other_players.players.get_mut(&pos.uid) {
                    p.x = pos.x;
                    p.y = pos.y;
                }
            }
        }

        // 8002: 玩家进入视野
        8002 => {
            if let Some(enter) = crate::codec::decode_player_enter(payload) {
                other_players.players.insert(
                    enter.uid,
                    OtherPlayerInfo {
                        uid: enter.uid,
                        name: enter.name,
                        x: enter.x,
                        y: enter.y,
                    },
                );
            }
        }

        // 8003: 玩家离开
        8003 => {
            if let Some(leave) = crate::codec::decode_player_leave(payload) {
                other_players.players.remove(&leave.uid);
            }
        }

        // 8004: 实体位置/属性 (proto)
        8004 => {
            if let Some(pos) = crate::codec::decode_entity_position(payload) {
                let entity_type = if pos.name.is_empty() {
                    "mob".to_string()
                } else {
                    String::new()
                };
                entities.entities.insert(
                    pos.entity_id,
                    EntityInfo {
                        entity_id: pos.entity_id,
                        def_id: pos.def_id,
                        name: pos.name,
                        x: pos.x,
                        y: pos.y,
                        hp: pos.hp,
                        max_hp: pos.max_hp,
                        level: pos.level,
                        entity_type,
                    },
                );
            }
        }

        // 9002: 实体列表 (proto)
        9002 => {
            if let Some(list) = crate::codec::decode_entity_list(payload) {
                for npc in &list.npcs {
                    entities.entities.insert(
                        npc.entity_id,
                        EntityInfo {
                            entity_id: npc.entity_id,
                            def_id: npc.def_id,
                            name: npc.name.clone(),
                            x: npc.x,
                            y: npc.y,
                            hp: npc.hp,
                            max_hp: npc.max_hp,
                            level: npc.level,
                            entity_type: npc.npc_type.clone(),
                        },
                    );
                }
                for mob in &list.mobs {
                    entities.entities.insert(
                        mob.entity_id,
                        EntityInfo {
                            entity_id: mob.entity_id,
                            def_id: mob.def_id,
                            name: mob.name.clone(),
                            x: mob.x,
                            y: mob.y,
                            hp: mob.hp,
                            max_hp: mob.max_hp,
                            level: mob.level,
                            entity_type: "mob".to_string(),
                        },
                    );
                }
            }
        }

        // 9100: 配置数据 (JSON)
        9100 => {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) {
                if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
                    game_config.items = items.clone();
                }
                if let Some(quests) = v.get("quests").and_then(|x| x.as_array()) {
                    game_config.quests = quests.clone();
                }
                game_config.loaded = true;
                info!("配置加载完成: {} 物品, {} 任务", game_config.items.len(), game_config.quests.len());
            }
        }

        _ => {
            debug!("未处理的消息 ID: {}", msg_id);
        }
    }
}

/// 解析 NPC 对话选项 JSON
fn parse_dialog_options(options_json: &str, _npc_id: u32) -> Vec<NpcDialogOption> {
    if options_json.is_empty() {
        return vec![NpcDialogOption {
            label: "关闭".to_string(),
            action: DialogAction::Close,
        }];
    }
    let arr: Vec<serde_json::Value> = match serde_json::from_str(options_json) {
        Ok(a) => a,
        Err(_) => {
            return vec![NpcDialogOption {
                label: "关闭".to_string(),
                action: DialogAction::Close,
            }]
        }
    };
    let mut options = Vec::new();
    for item in arr {
        let label = item
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("关闭")
            .to_string();
        let typ = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("close");
        let quest_id = item
            .get("questId")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let action = match typ {
            "accept" => DialogAction::AcceptQuest(quest_id),
            "complete" => DialogAction::CompleteQuest(quest_id),
            "shop" => DialogAction::OpenShop,
            "close" => DialogAction::Close,
            _ => DialogAction::None,
        };
        options.push(NpcDialogOption { label, action });
    }
    if options.is_empty() {
        options.push(NpcDialogOption {
            label: "关闭".to_string(),
            action: DialogAction::Close,
        });
    }
    options
}

// ============================================================================
// 键盘移动系统
// ============================================================================

/// 移动系统: WASD 键盘输入，发送移动消息
///
/// 100ms 节流避免消息过多
pub fn movement_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    net: Res<NetworkResource>,
    player: Res<PlayerState>,
    mut input: ResMut<InputState>,
    dialog: Res<NpcDialogState>,
) {
    if !player.logged_in || !net.is_connected() {
        return;
    }
    // 对话打开时禁止移动
    if dialog.dialog.is_some() {
        return;
    }

    let mut dx = 0.0f32;
    let mut dy = 0.0f32;

    if keyboard.pressed(KeyCode::KeyW) {
        dy -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        dy += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        dx -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        dx += 1.0;
    }

    if dx == 0.0 && dy == 0.0 {
        return;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    if now - input.last_move_time < 100 {
        return;
    }
    input.last_move_time = now;

    // 归一化方向向量
    let len = (dx * dx + dy * dy).sqrt();
    if len > 0.0 {
        dx /= len;
        dy /= len;
    }

    let speed = 5.0;
    let new_x = player.x + dx * speed;
    let new_y = player.y + dy * speed;

    // 方向: 0=上, 1=右, 2=下, 3=左
    let dir = if dy < 0.0 {
        0
    } else if dx > 0.0 {
        1
    } else if dy > 0.0 {
        2
    } else {
        3
    };

    let payload = crate::codec::encode_move(new_x, new_y, dir);
    net.send(NetworkCommand::Send {
        msg_id: 3001,
        payload,
    });
}

// ============================================================================
// 鼠标输入系统
// ============================================================================

/// 鼠标点击: 左键攻击怪物/NPC交互/拾取, 右键移动到点击位置
pub fn mouse_input_system(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    net: Res<NetworkResource>,
    player: Res<PlayerState>,
    entities: Res<EntityManager>,
    drops: Res<DropManager>,
    mut target: ResMut<TargetEntity>,
    mut input: ResMut<InputState>,
    dialog: Res<NpcDialogState>,
) {
    if !player.logged_in || !net.is_connected() {
        return;
    }
    // 对话打开时禁止点击攻击
    if dialog.dialog.is_some() {
        return;
    }

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let window = if let Ok(w) = windows.get_single() {
        w
    } else {
        return;
    };
    let (camera, camera_transform) = if let Ok(c) = camera_q.get_single() {
        c
    } else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Some(world_pos) = camera.viewport_to_world_2d(camera_transform, cursor_pos) else {
        return;
    };
    // world_pos 是 Bevy 坐标 (y 已翻转)，转回游戏坐标
    let game_x = world_pos.x;
    let game_y = -world_pos.y;

    // 查找最近的实体 (优先级: 怪物 > NPC > 掉落物), 范围 40px
    let mut best_ent: Option<(u64, f32, bool)> = None; // (id, dist, is_mob)
    for (eid, info) in &entities.entities {
        let dx = info.x - game_x;
        let dy = info.y - game_y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 40.0 {
            let is_mob = !info.is_npc();
            if best_ent.is_none()
                || (is_mob && !best_ent.unwrap().2)
                || dist < best_ent.unwrap().1
            {
                best_ent = Some((*eid, dist, is_mob));
            }
        }
    }

    // 查找最近的掉落物
    let mut best_drop: Option<(u64, f32)> = None;
    for (did, drop) in &drops.drops {
        let dx = drop.x - game_x;
        let dy = drop.y - game_y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 30.0 && (best_drop.is_none() || dist < best_drop.unwrap().1) {
            best_drop = Some((*did, dist));
        }
    }

    // 优先攻击怪物, 其次 NPC 交互, 最后拾取
    if let Some((eid, _, is_mob)) = best_ent {
        if is_mob {
            // 攻击怪物 — 节流 300ms
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if now - input.last_attack_time < 300 {
                return;
            }
            input.last_attack_time = now;

            target.entity_id = Some(eid);
            target.is_mob = true;
            let payload = crate::codec::encode_attack(eid);
            net.send(NetworkCommand::Send {
                msg_id: 1001,
                payload,
            });
        } else {
            // NPC 交互
            let payload = crate::codec::encode_npc_interact(eid as u32);
            net.send(NetworkCommand::Send {
                msg_id: 1007,
                payload,
            });
        }
        return;
    }

    if let Some((did, _)) = best_drop {
        let payload = crate::codec::encode_pickup(did);
        net.send(NetworkCommand::Send {
            msg_id: 1003,
            payload,
        });
        return;
    }

    // 没有点中实体 — 左键移动到点击位置
    let dx = game_x - player.x;
    let dy = game_y - player.y;
    let dir = if dy < 0.0 {
        0
    } else if dx > 0.0 {
        1
    } else if dy > 0.0 {
        2
    } else {
        3
    };
    let payload = crate::codec::encode_move(game_x, game_y, dir);
    net.send(NetworkCommand::Send {
        msg_id: 3001,
        payload,
    });
}

// ============================================================================
// 面板切换系统
// ============================================================================

/// 面板切换: I=背包, Q=任务, L=战斗日志
pub fn panel_toggle_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panels: ResMut<PanelVisibility>,
    dialog: Res<NpcDialogState>,
) {
    if keyboard.just_pressed(KeyCode::KeyI) {
        panels.inventory = !panels.inventory;
    }
    if keyboard.just_pressed(KeyCode::KeyQ) {
        panels.quest = !panels.quest;
    }
    if keyboard.just_pressed(KeyCode::KeyL) {
        panels.combat_log = !panels.combat_log;
    }
    // ESC 关闭对话由 ui::update_dialog_system 处理
    let _ = dialog;
}

// ============================================================================
// 渲染系统
// ============================================================================

/// 渲染系统: 同步游戏实体到 Bevy ECS 实体
///
/// - 玩家: 绿色方块 (始终可见，即使未登录也在 0,0)
/// - 其他玩家: 蓝色方块
/// - 怪物: 红色方块
/// - NPC: 黄色方块
/// - 选中目标: 白色光环
/// - 掉落物: 金色小方块
/// - HP 条: 实体上方
pub fn render_system(
    player: Res<PlayerState>,
    entities: Res<EntityManager>,
    other_players: Res<OtherPlayerManager>,
    drops: Res<DropManager>,
    target: Res<TargetEntity>,
    mut commands: Commands,
    player_query: Query<Entity, With<Player>>,
    entity_query: Query<(Entity, &GameEntity)>,
    other_query: Query<(Entity, &OtherPlayer)>,
    drop_query: Query<(Entity, &DroppedItem)>,
    ring_query: Query<Entity, With<SelectionRing>>,
) {
    // --- 玩家自身 (始终渲染) ---
    let player_color = if player.logged_in {
        Color::srgb(0.2, 0.95, 0.2)
    } else {
        Color::srgb(0.4, 0.6, 0.4) // 未登录时暗色
    };
    if let Some(player_entity) = player_query.iter().next() {
        // 更新位置和颜色
        commands.entity(player_entity).insert(Transform::from_xyz(
            player.x,
            -player.y,
            10.0,
        ));
    } else {
        // 始终创建玩家实体
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: player_color,
                    custom_size: Some(Vec2::new(32.0, 32.0)),
                    ..default()
                },
                transform: Transform::from_xyz(player.x, -player.y, 10.0),
                ..default()
            },
            Player,
            GamePosition::new(player.x, player.y),
            HealthBar::new(player.hp, player.max_hp),
        ));
    }
    // 更新玩家 HP 条
    if let Some(player_entity) = player_query.iter().next() {
        commands.entity(player_entity).insert(HealthBar::new(player.hp, player.max_hp));
    }

    // --- 其他玩家 ---
    for (entity, other) in other_query.iter() {
        if !other_players.players.contains_key(&other.uid) {
            commands.entity(entity).despawn();
        }
    }
    for (uid, info) in &other_players.players {
        let mut found = false;
        for (entity, other) in other_query.iter() {
            if other.uid == *uid {
                commands.entity(entity).insert(Transform::from_xyz(
                    info.x,
                    -info.y,
                    5.0,
                ));
                found = true;
                break;
            }
        }
        if !found {
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::srgb(0.2, 0.5, 0.9),
                        custom_size: Some(Vec2::new(28.0, 28.0)),
                        ..default()
                    },
                    transform: Transform::from_xyz(info.x, -info.y, 5.0),
                    ..default()
                },
                OtherPlayer {
                    uid: info.uid,
                    name: info.name.clone(),
                },
                GamePosition::new(info.x, info.y),
            ));
        }
    }

    // --- 游戏实体 (怪物/NPC) ---
    for (entity, game_ent) in entity_query.iter() {
        if !entities.entities.contains_key(&game_ent.entity_id) {
            commands.entity(entity).despawn();
        }
    }
    for (eid, info) in &entities.entities {
        let mut found = false;
        for (entity, game_ent) in entity_query.iter() {
            if game_ent.entity_id == *eid {
                commands.entity(entity).insert(Transform::from_xyz(info.x, -info.y, 5.0));
                commands.entity(entity).insert(HealthBar::new(info.hp, info.max_hp));
                found = true;
                break;
            }
        }
        if !found {
            let (size, color) = if info.is_npc() {
                (32.0, Color::srgb(0.95, 0.85, 0.2)) // NPC: 亮黄色
            } else {
                (26.0, Color::srgb(0.95, 0.25, 0.25)) // 怪物: 亮红色
            };
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color,
                        custom_size: Some(Vec2::new(size, size)),
                        ..default()
                    },
                    transform: Transform::from_xyz(info.x, -info.y, 5.0),
                    ..default()
                },
                GameEntity {
                    entity_id: info.entity_id,
                    def_id: info.def_id,
                    name: info.name.clone(),
                    entity_type: if info.is_npc() {
                        EntityType::Npc
                    } else {
                        EntityType::Mob
                    },
                },
                GamePosition::new(info.x, info.y),
                HealthBar::new(info.hp, info.max_hp),
            ));
        }
    }

    // --- 掉落物 ---
    for (entity, drop) in drop_query.iter() {
        if !drops.drops.contains_key(&drop.drop_id) {
            commands.entity(entity).despawn();
        }
    }
    for (did, drop) in &drops.drops {
        let mut found = false;
        for (entity, drop_comp) in drop_query.iter() {
            if drop_comp.drop_id == *did {
                commands.entity(entity).insert(Transform::from_xyz(drop.x, -drop.y, 3.0));
                found = true;
                break;
            }
        }
        if !found {
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::srgb(1.0, 0.85, 0.0),
                        custom_size: Some(Vec2::new(14.0, 14.0)),
                        ..default()
                    },
                    transform: Transform::from_xyz(drop.x, -drop.y, 3.0),
                    ..default()
                },
                DroppedItem {
                    drop_id: drop.drop_id,
                    item_id: drop.item_id,
                    count: drop.count,
                },
            ));
        }
    }

    // --- 选中目标光环 ---
    // 移除旧光环
    for entity in ring_query.iter() {
        commands.entity(entity).despawn();
    }
    // 创建新光环
    if let Some(target_id) = target.entity_id {
        if target.is_mob {
            if let Some(info) = entities.entities.get(&target_id) {
                commands.spawn((
                    SpriteBundle {
                        sprite: Sprite {
                            color: Color::srgb(1.0, 1.0, 1.0),
                            custom_size: Some(Vec2::new(40.0, 40.0)),
                            ..default()
                        },
                        transform: Transform::from_xyz(info.x, -info.y, 4.0),
                        ..default()
                    },
                    SelectionRing {
                        entity_id: target_id,
                    },
                ));
            }
        }
    }
}

// ============================================================================
// 相机跟随系统
// ============================================================================

/// 相机跟随玩家
pub fn camera_follow_system(
    player: Res<PlayerState>,
    mut camera_query: Query<&mut Transform, (With<Camera2d>, Without<Player>)>,
) {
    if !player.logged_in {
        return;
    }
    for mut transform in camera_query.iter_mut() {
        // 平滑跟随
        transform.translation.x = transform.translation.x.lerp(player.x, 0.15);
        transform.translation.y = transform.translation.y.lerp(-player.y, 0.15);
    }
}

// ============================================================================
// 定时查询系统
// ============================================================================

/// 定时查询实体列表 (触发 mob AI tick + 获取最新实体位置)
///
/// 每 500ms 发送一次 4002 查询
pub fn entity_query_timer(
    time: Res<Time>,
    net: Res<NetworkResource>,
    player: Res<PlayerState>,
    mut timer: Local<Option<Timer>>,
) {
    if !player.logged_in || !net.is_connected() {
        return;
    }
    let timer = timer.get_or_insert_with(|| Timer::from_seconds(0.5, TimerMode::Repeating));
    timer.tick(time.delta());
    if timer.just_finished() {
        let payload = crate::codec::encode_query_entities();
        net.send(NetworkCommand::Send {
            msg_id: 4002,
            payload,
        });
    }
}
