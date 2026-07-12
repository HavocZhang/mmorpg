//! ECS 系统定义
//!
//! 包含:
//! - handle_server_message: 服务器下行消息分发
//! - movement_system: WASD 键盘移动
//! - mouse_input_system: 鼠标点击攻击/NPC交互/拾取
//! - panel_toggle_system: I/Q/L 键切换面板
//! - render_system: 同步游戏实体到 Bevy ECS (父子层级: 主体+HP条+名称)
//! - update_hp_bar_system: 更新 HP 条前景宽度
//! - damage_text_system: 伤害飘字动画 + 自动销毁
//! - spawn_damage_text_system: 消费 DamageEvent 生成飘字
//! - camera_follow_system: 相机跟随玩家
//! - camera_zoom_system: 滚轮缩放
//! - death_system: 玩家死亡处理

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::view::{InheritedVisibility, NoFrustumCulling, ViewVisibility};
use bevy::text::JustifyText;

use crate::components::*;
use crate::network::{NetworkCommand, NetworkResource};
use crate::resources::*;

// ============================================================================
// 常量
// ============================================================================

/// HP 条宽度
const HP_BAR_W: f32 = 52.0;
/// HP 条高度
const HP_BAR_H: f32 = 5.0;
/// 实体方块大小
const ENTITY_SIZE: f32 = 36.0;
/// 玩家方块大小
const PLAYER_SIZE: f32 = 40.0;

// ============================================================================
// 世界初始化
// ============================================================================

/// 世界网格标记
#[derive(Component)]
pub struct WorldGrid;

/// 创建世界背景: 网格 + 边界标记
pub fn setup_world(mut commands: Commands) {
    const GRID_SIZE: i32 = 20;
    const CELL: f32 = 80.0;
    const HALF: f32 = (GRID_SIZE as f32) * CELL / 2.0; // 800.0
    const MAJOR_EVERY: i32 = 4; // 每 4 格一条主线 (320px)

    // 细网格线 (低对比度)
    for i in 0..=GRID_SIZE {
        let is_major = i % MAJOR_EVERY == 0;
        let color = if is_major {
            Color::srgba(0.22, 0.22, 0.35, 0.6)
        } else {
            Color::srgba(0.12, 0.12, 0.20, 0.35)
        };
        let line_w = if is_major { 1.5 } else { 1.0 };

        // 竖线
        let x = -HALF + i as f32 * CELL;
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color,
                    custom_size: Some(Vec2::new(line_w, GRID_SIZE as f32 * CELL)),
                    ..default()
                },
                transform: Transform::from_xyz(x, 0.0, 0.0),
                ..default()
            },
            WorldGrid,
        ));
        // 横线
        let y = -HALF + i as f32 * CELL;
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color,
                    custom_size: Some(Vec2::new(GRID_SIZE as f32 * CELL, line_w)),
                    ..default()
                },
                transform: Transform::from_xyz(0.0, y, 0.0),
                ..default()
            },
            WorldGrid,
        ));
    }

    // 原点标记 (红十字)
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::srgb(0.6, 0.25, 0.25),
                custom_size: Some(Vec2::new(60.0, 3.0)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 0.1),
            ..default()
        },
        WorldGrid,
    ));
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::srgb(0.6, 0.25, 0.25),
                custom_size: Some(Vec2::new(3.0, 60.0)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 0.1),
            ..default()
        },
        WorldGrid,
    ));

    // 世界边界框
    const WORLD_HALF: f32 = 800.0;
    let border_color = Color::srgb(0.45, 0.35, 0.22);
    for (pos, size) in [
        (Vec3::new(0.0, WORLD_HALF, 0.0), Vec2::new(WORLD_HALF * 2.0, 5.0)),
        (Vec3::new(0.0, -WORLD_HALF, 0.0), Vec2::new(WORLD_HALF * 2.0, 5.0)),
        (Vec3::new(-WORLD_HALF, 0.0, 0.0), Vec2::new(5.0, WORLD_HALF * 2.0)),
        (Vec3::new(WORLD_HALF, 0.0, 0.0), Vec2::new(5.0, WORLD_HALF * 2.0)),
    ] {
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: border_color,
                    custom_size: Some(size),
                    ..default()
                },
                transform: Transform::from_translation(pos),
                ..default()
            },
            WorldGrid,
        ));
    }

    info!("世界网格已生成 ({}x{}, cell={}px, 边界 {}x{})", GRID_SIZE, GRID_SIZE, CELL, WORLD_HALF * 2.0, WORLD_HALF * 2.0);
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
    damage_events: &mut bevy::ecs::event::Events<DamageEvent>,
    exp_events: &mut bevy::ecs::event::Events<ExpGainEvent>,
    death_events: &mut bevy::ecs::event::Events<PlayerDeathEvent>,
) {
    match msg_id {
        // 5001: 玩家属性
        5001 => {
            if let Some(stats) = crate::codec::decode_player_stats(payload) {
                let was_dead = player.hp <= 0 && player.logged_in;
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
                // 死亡检测
                if player.hp <= 0 && !was_dead {
                    death_events.send(PlayerDeathEvent);
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
                        exp_events.send(ExpGainEvent { amount: update.gained });
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

        // 6001: 战斗结果 → 发送 DamageEvent
        6001 => {
            if let Some(result) = crate::codec::decode_combat_result(payload) {
                if !result.error.is_empty() {
                    combat_log.push(format!("战斗错误: {}", result.error));
                } else if result.miss {
                    // 未命中: 在目标位置发 MISS 飘字
                    if let Some(info) = entities.entities.get(&result.target_uid) {
                        damage_events.send(DamageEvent {
                            target_entity_id: result.target_uid,
                            world_x: info.x,
                            world_y: info.y,
                            damage: 0,
                            is_crit: false,
                            is_miss: true,
                        });
                    }
                    combat_log.push(format!("攻击 {} 未命中!", result.target_name));
                } else {
                    // 命中: 发伤害飘字
                    if let Some(info) = entities.entities.get(&result.target_uid) {
                        damage_events.send(DamageEvent {
                            target_entity_id: result.target_uid,
                            world_x: info.x,
                            world_y: info.y,
                            damage: result.damage,
                            is_crit: result.crit,
                            is_miss: false,
                        });
                    }
                    if result.swing {
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

        // 8001: 玩家位置更新
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

        // 8004: 实体位置/属性
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

        // 9002: 实体列表
        9002 => {
            if let Some(list) = crate::codec::decode_entity_list(payload) {
                debug!("收到实体列表: {} NPC, {} 怪物", list.npcs.len(), list.mobs.len());
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

        // 9100: 配置数据
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

/// 移动系统: WASD 键盘输入，发送移动消息 (100ms 节流)
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
    // 玩家死亡或对话打开时禁止移动
    if player.hp <= 0 || dialog.dialog.is_some() {
        return;
    }

    let mut dx = 0.0f32;
    let mut dy = 0.0f32;

    if keyboard.pressed(KeyCode::KeyW) {
        dy -= 1.0; // 游戏坐标 y 向下，W 向上 = y 减小
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

    if now - input.last_move_time < 50 {
        return;
    }
    input.last_move_time = now;

    // 归一化方向向量
    let len = (dx * dx + dy * dy).sqrt();
    if len > 0.0 {
        dx /= len;
        dy /= len;
    }

    let speed = 4.0;
    let new_x = player.x + dx * speed;
    let new_y = player.y + dy * speed;

    // 边界限制
    let new_x = new_x.clamp(-790.0, 790.0);
    let new_y = new_y.clamp(-790.0, 790.0);

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

/// 鼠标点击: 左键攻击怪物/NPC交互/拾取
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
    if player.hp <= 0 || dialog.dialog.is_some() {
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
    // Bevy 世界坐标 → 游戏坐标 (y 翻转)
    let game_x = world_pos.x;
    let game_y = -world_pos.y;

    // 查找最近的实体，范围 40px
    let mut best_ent: Option<(u64, f32, bool)> = None;
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
    let new_x = game_x.clamp(-790.0, 790.0);
    let new_y = game_y.clamp(-790.0, 790.0);
    let dx = new_x - player.x;
    let dy = new_y - player.y;
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
// 面板切换系统
// ============================================================================

/// 面板切换: I=背包, Q=任务, L=战斗日志
pub fn panel_toggle_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panels: ResMut<PanelVisibility>,
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
}

// ============================================================================
// 渲染系统 (父子层级结构)
// ============================================================================

/// 渲染系统: 同步游戏实体到 Bevy ECS
///
/// 结构: 实体主体 (Sprite) + 子实体 (HP条背景 + HP条前景 + 名称标签)
/// 父实体移动时子实体自动跟随
pub fn render_system(
    player: Res<PlayerState>,
    entities: Res<EntityManager>,
    other_players: Res<OtherPlayerManager>,
    drops: Res<DropManager>,
    target: Res<TargetEntity>,
    game_font: Res<GameFont>,
    mut commands: Commands,
    player_query: Query<Entity, With<Player>>,
    entity_query: Query<(Entity, &GameEntity)>,
    other_query: Query<(Entity, &OtherPlayer)>,
    drop_query: Query<(Entity, &DroppedItem)>,
    ring_query: Query<Entity, With<SelectionRing>>,
) {
    let font = &game_font.font;
    // 诊断日志: 登录后打印实体坐标 (前 5 次)
    static RENDER_DIAG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if player.logged_in && !entities.entities.is_empty() {
        let diag_count = RENDER_DIAG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if diag_count < 5 {
            let sample: Vec<(u64, f32, f32, &str)> = entities.entities.iter()
                .take(3)
                .map(|(eid, info)| (*eid, info.x, info.y, info.name.as_str()))
                .collect();
            info!(
                "[DIAG] render_system: player=({},{}) entities={} game_entities={} sample={:?}",
                player.x, player.y,
                entities.entities.len(),
                entity_query.iter().count(),
                sample,
            );
        }
    }
    // ── 玩家自身 (始终渲染) ──
    let player_color = if player.hp <= 0 && player.logged_in {
        Color::srgb(0.3, 0.3, 0.3) // 死亡时灰色
    } else if player.logged_in {
        Color::srgb(0.2, 0.95, 0.2)
    } else {
        Color::srgb(0.4, 0.6, 0.4)
    };

    if let Some(player_entity) = player_query.iter().next() {
        // 只更新目标位置和 HP，Transform 由插值系统处理
        commands
            .entity(player_entity)
            .insert(TargetPosition::new(player.x, player.y))
            .insert(HealthBar::new(0, player.hp, player.max_hp));
    } else {
        // 首次创建玩家实体 (带 HP 条和名称)
        let entity = spawn_entity_with_attachments(
            &mut commands,
            player.x,
            player.y,
            player_color,
            PLAYER_SIZE,
            0, // entity_id=0 表示玩家自身
            &player.name,
            true,
            player.hp,
            player.max_hp,
            font,
        );
        commands.entity(entity).insert(Player);
    }

    // ── 其他玩家 ──
    // 移除已离开的
    for (entity, other) in other_query.iter() {
        if !other_players.players.contains_key(&other.uid) {
            commands.entity(entity).despawn_recursive();
        }
    }
    // 更新或创建
    for (uid, info) in &other_players.players {
        let mut found = false;
        for (entity, other) in other_query.iter() {
            if other.uid == *uid {
                commands
                    .entity(entity)
                    .insert(TargetPosition::new(info.x, info.y));
                found = true;
                break;
            }
        }
        if !found {
            let entity = spawn_entity_with_attachments(
                &mut commands,
                info.x,
                info.y,
                Color::srgb(0.2, 0.5, 0.9),
                ENTITY_SIZE,
                *uid,
                &info.name,
                false,
                100,
                100, // 其他玩家暂无 HP 数据，默认满血
                font,
            );
            commands.entity(entity).insert(OtherPlayer {
                uid: info.uid,
                name: info.name.clone(),
            });
        }
    }

    // ── 游戏实体 (怪物/NPC) ──
    for (entity, game_ent) in entity_query.iter() {
        if !entities.entities.contains_key(&game_ent.entity_id) {
            commands.entity(entity).despawn_recursive();
        }
    }
    for (eid, info) in &entities.entities {
        let mut found = false;
        for (entity, game_ent) in entity_query.iter() {
            if game_ent.entity_id == *eid {
                commands
                    .entity(entity)
                    .insert(TargetPosition::new(info.x, info.y))
                    .insert(HealthBar::new(*eid, info.hp, info.max_hp));
                found = true;
                break;
            }
        }
        if !found {
            let (size, color) = if info.is_npc() {
                (ENTITY_SIZE, Color::srgb(0.95, 0.85, 0.2))
            } else {
                (ENTITY_SIZE, Color::srgb(0.95, 0.25, 0.25))
            };
            let entity = spawn_entity_with_attachments(
                &mut commands,
                info.x,
                info.y,
                color,
                size,
                *eid,
                &info.name,
                false,
                info.hp,
                info.max_hp,
                font,
            );
            commands.entity(entity).insert(GameEntity {
                entity_id: info.entity_id,
                def_id: info.def_id,
                name: info.name.clone(),
                entity_type: if info.is_npc() {
                    EntityType::Npc
                } else {
                    EntityType::Mob
                },
            });
        }
    }

    // ── 掉落物 ──
    for (entity, drop) in drop_query.iter() {
        if !drops.drops.contains_key(&drop.drop_id) {
            commands.entity(entity).despawn();
        }
    }
    for (did, drop) in &drops.drops {
        let mut found = false;
        for (entity, drop_comp) in drop_query.iter() {
            if drop_comp.drop_id == *did {
                commands
                    .entity(entity)
                    .insert(Transform::from_xyz(drop.x, -drop.y, 3.0));
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

    // ── 选中目标光环 ──
    for entity in ring_query.iter() {
        commands.entity(entity).despawn();
    }
    if let Some(target_id) = target.entity_id {
        if target.is_mob {
            if let Some(info) = entities.entities.get(&target_id) {
                commands.spawn((
                    SpriteBundle {
                        sprite: Sprite {
                            color: Color::srgba(1.0, 1.0, 1.0, 0.6),
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

/// 生成带 HP 条和名称标签的实体 (父子层级结构)
///
/// 返回父实体 Entity
fn spawn_entity_with_attachments(
    commands: &mut Commands,
    game_x: f32,
    game_y: f32,
    color: Color,
    size: f32,
    entity_id: u64,
    name: &str,
    is_player: bool,
    hp: i32,
    max_hp: i32,
    font: &Handle<Font>,
) -> Entity {
    let bevy_y = -game_y;
    let z = if is_player { 10.0 } else { 5.0 };

    // 父实体: 主体方块
    // NoFrustumCulling: 禁用视锥体剔除，确保实体始终进入渲染管线
    let entity = commands
        .spawn((
            SpriteBundle {
                sprite: Sprite {
                    color,
                    custom_size: Some(Vec2::new(size, size)),
                    ..default()
                },
                transform: Transform::from_xyz(game_x, bevy_y, z),
                ..default()
            },
            NoFrustumCulling,
            GamePosition::new(game_x, game_y),
            TargetPosition::new(game_x, game_y),
            HealthBar::new(entity_id, hp, max_hp),
        ))
        .with_children(|parent| {
            // 子1: HP 条背景 (黑色底)
            parent
                .spawn((
                    SpriteBundle {
                        sprite: Sprite {
                            color: Color::srgb(0.1, 0.1, 0.1),
                            custom_size: Some(Vec2::new(HP_BAR_W, HP_BAR_H)),
                            ..default()
                        },
                        transform: Transform::from_xyz(0.0, size / 2.0 + 8.0, 0.1),
                        ..default()
                    },
                    HpBarMarker { entity_id },
                ))
                .with_children(|hpbar| {
                    // 孙: HP 条前景 (绿色填充)
                    let ratio = (hp as f32 / max_hp as f32).clamp(0.0, 1.0);
                    hpbar.spawn((
                        SpriteBundle {
                            sprite: Sprite {
                                color: Color::srgb(0.2, 0.9, 0.2),
                                custom_size: Some(Vec2::new(HP_BAR_W * ratio, HP_BAR_H)),
                                ..default()
                            },
                            // 左对齐: 中心 = -max_w/2 + width/2
                            transform: Transform::from_xyz(
                                -HP_BAR_W / 2.0 + (HP_BAR_W * ratio) / 2.0,
                                0.0,
                                0.1,
                            ),
                            ..default()
                        },
                        HpBarFill {
                            entity_id,
                            max_width: HP_BAR_W,
                        },
                    ));
                });

            // 子2: 名称标签 (Text2dBundle)
            if !name.is_empty() {
                let name_color = if is_player {
                    Color::srgb(0.9, 1.0, 0.9)
                } else if entity_id > 0 && entity_id < 100000 {
                    Color::srgb(1.0, 0.95, 0.6) // NPC: 暖黄
                } else {
                    Color::srgb(1.0, 0.7, 0.7) // 怪物: 暖红
                };
                parent.spawn((
                    Text2dBundle {
                        text: Text {
                            sections: vec![TextSection::new(
                                name.to_string(),
                                TextStyle {
                                    font: font.clone(),
                                    font_size: 16.0,
                                    color: name_color,
                                    ..default()
                                },
                            )],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        transform: Transform::from_xyz(0.0, size / 2.0 + 22.0, 0.2),
                        ..default()
                    },
                    NameTag { entity_id },
                ));
            }
        })
        .id();

    entity
}

// ============================================================================
// HP 条更新系统
// ============================================================================

/// 更新 HP 条前景宽度 (HP 变化时)
///
/// 通过 entity_id 匹配 HealthBar 和 HpBarFill
pub fn update_hp_bar_system(
    health_query: Query<&HealthBar>,
    mut fill_query: Query<(&HpBarFill, &mut Sprite, &mut Transform)>,
) {
    // 构建 entity_id -> (hp, max_hp) 缓存
    let mut health_map: std::collections::HashMap<u64, (i32, i32)> = std::collections::HashMap::new();
    for health in health_query.iter() {
        health_map.insert(health.entity_id, (health.hp, health.max_hp));
    }

    for (fill, mut sprite, mut tf) in fill_query.iter_mut() {
        let Some(&(hp, max_hp)) = health_map.get(&fill.entity_id) else {
            continue;
        };
        let ratio = if max_hp <= 0 {
            0.0
        } else {
            (hp as f32 / max_hp as f32).clamp(0.0, 1.0)
        };
        // HP 颜色: 高=绿, 中=黄, 低=红
        let color = if ratio > 0.5 {
            Color::srgb(0.2, 0.9, 0.2)
        } else if ratio > 0.25 {
            Color::srgb(0.9, 0.9, 0.2)
        } else {
            Color::srgb(0.9, 0.2, 0.2)
        };
        sprite.color = color;
        if let Some(size) = sprite.custom_size.as_mut() {
            size.x = fill.max_width * ratio;
        }
        // 保持左对齐
        tf.translation.x = -fill.max_width / 2.0 + (fill.max_width * ratio) / 2.0;
    }
}

// ============================================================================
// 伤害飘字系统
// ============================================================================

/// 消费 DamageEvent 生成伤害飘字
pub fn spawn_damage_text_system(
    mut events: EventReader<DamageEvent>,
    game_font: Res<GameFont>,
    mut commands: Commands,
) {
    for event in events.read() {
        let bevy_y = -event.world_y;
        let (text, color, size) = if event.is_miss {
            ("MISS".to_string(), Color::srgb(0.7, 0.7, 0.7), 18.0)
        } else if event.is_crit {
            (format!("-{}", event.damage), Color::srgb(1.0, 0.8, 0.0), 28.0)
        } else {
            (format!("-{}", event.damage), Color::srgb(1.0, 0.3, 0.3), 20.0)
        };

        commands.spawn((
            Text2dBundle {
                text: Text {
                    sections: vec![TextSection::new(
                        text,
                        TextStyle {
                            font: game_font.font.clone(),
                            font_size: size,
                            color,
                            ..default()
                        },
                    )],
                    justify: JustifyText::Center,
                    ..default()
                },
                transform: Transform::from_xyz(event.world_x, bevy_y + 30.0, 30.0),
                ..default()
            },
            DamageText {
                timer: Timer::from_seconds(0.8, TimerMode::Once),
                start_y: bevy_y + 30.0,
            },
        ));
    }
}

/// 伤害飘字动画: 上升 + 淡出 + 自动销毁
pub fn damage_text_system(
    time: Res<Time>,
    mut query: Query<(Entity, &mut DamageText, &mut Transform, &mut Text)>,
    mut commands: Commands,
) {
    for (entity, mut dmg, mut tf, mut text) in query.iter_mut() {
        dmg.timer.tick(time.delta());
        let elapsed = dmg.timer.elapsed_secs();
        let duration = dmg.timer.duration().as_secs_f32();
        let t = (elapsed / duration).clamp(0.0, 1.0);

        // 上升 40px
        tf.translation.y = dmg.start_y + t * 40.0;
        // 淡出
        if let Some(section) = text.sections.get_mut(0) {
            section.style.color.set_alpha(1.0 - t);
        }

        if dmg.timer.just_finished() {
            commands.entity(entity).despawn();
        }
    }
}

// ============================================================================
// 经验飘字系统
// ============================================================================

/// 消费 ExpGainEvent 生成经验飘字
pub fn spawn_exp_text_system(
    mut events: EventReader<ExpGainEvent>,
    player: Res<PlayerState>,
    game_font: Res<GameFont>,
    mut commands: Commands,
) {
    for event in events.read() {
        commands.spawn((
            Text2dBundle {
                text: Text {
                    sections: vec![TextSection::new(
                        format!("+{} EXP", event.amount),
                        TextStyle {
                            font: game_font.font.clone(),
                            font_size: 18.0,
                            color: Color::srgb(0.3, 0.9, 1.0),
                            ..default()
                        },
                    )],
                    justify: JustifyText::Center,
                    ..default()
                },
                transform: Transform::from_xyz(player.x, -player.y + 40.0, 30.0),
                ..default()
            },
            DamageText {
                timer: Timer::from_seconds(1.2, TimerMode::Once),
                start_y: -player.y + 40.0,
            },
        ));
    }
}

// ============================================================================
// 位置插值系统 (丝滑移动)
// ============================================================================

/// 将实体的 Transform 朝 TargetPosition 平滑插值
///
/// - 首次 (initialized=true 且 Transform 距离目标很远): 直接吸附，避免从原点飘过来
/// - 后续: 按 lerp 因子平滑过渡
///
/// lerp 因子 0.18 → 约 5-6 帧到达目标 (60fps 下约 100ms)，既丝滑又不滞后
pub fn interpolate_position_system(
    mut query: Query<(&TargetPosition, &mut Transform), Or<(With<Player>, With<GameEntity>, With<OtherPlayer>)>>,
) {
    for (target, mut transform) in query.iter_mut() {
        let target_x = target.x;
        let target_y = -target.y; // 游戏坐标 y 向下 → Bevy y 向上

        let dx = target_x - transform.translation.x;
        let dy = target_y - transform.translation.y;
        let dist_sq = dx * dx + dy * dy;

        // 首次初始化或距离过大 (>200) → 直接吸附，避免长距离飘移
        if !target.initialized || dist_sq > 200.0 * 200.0 {
            transform.translation.x = target_x;
            transform.translation.y = target_y;
        } else if dist_sq > 0.01 {
            // 平滑插值 (lerp 因子越大越快)
            transform.translation.x = transform.translation.x.lerp(target_x, 0.18);
            transform.translation.y = transform.translation.y.lerp(target_y, 0.18);
        }
    }
}

// ============================================================================
// 可见性诊断
// ============================================================================

/// 诊断系统: 检查 GameEntity 的可见性状态
/// 运行在 PostUpdate (CheckVisibility 之后)，打印 ViewVisibility/InheritedVisibility/GlobalTransform
pub fn visibility_diagnostic_system(
    player: Res<PlayerState>,
    game_entities: Query<(
        &GameEntity,
        &GlobalTransform,
        &InheritedVisibility,
        &ViewVisibility,
    )>,
) {
    if !player.logged_in || game_entities.is_empty() {
        return;
    }
    static VIS_DIAG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = VIS_DIAG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n >= 10 {
        return;
    }
    for (ge, gt, inh, view) in game_entities.iter().take(5) {
        let pos = gt.translation();
        info!(
            "[VIS-DIAG] {} id={} pos=({:.1},{:.1},{:.1}) inherited={} view={}",
            ge.name, ge.entity_id, pos.x, pos.y, pos.z, inh.get(), view.get()
        );
    }
    let total = game_entities.iter().count();
    let visible = game_entities.iter().filter(|(_, _, _, v)| v.get()).count();
    let inherited = game_entities.iter().filter(|(_, _, i, _)| i.get()).count();
    info!(
        "[VIS-DIAG] total={} inherited={} visible={} (frame {})",
        total, inherited, visible, n
    );
}

// ============================================================================
// 相机系统
// ============================================================================

/// 相机跟随玩家 (平滑 lerp，远距离直接吸附)
pub fn camera_follow_system(
    player: Res<PlayerState>,
    mut camera_query: Query<&mut Transform, (With<Camera2d>, Without<Player>)>,
) {
    if !player.logged_in {
        return;
    }
    let target_x = player.x;
    let target_y = -player.y;
    for mut transform in camera_query.iter_mut() {
        let dx = target_x - transform.translation.x;
        let dy = target_y - transform.translation.y;
        let dist_sq = dx * dx + dy * dy;
        // 距离很远 (>300) → 直接跳到玩家位置，避免长时间飘移
        if dist_sq > 300.0 * 300.0 {
            transform.translation.x = target_x;
            transform.translation.y = target_y;
        } else {
            transform.translation.x = transform.translation.x.lerp(target_x, 0.15);
            transform.translation.y = transform.translation.y.lerp(target_y, 0.15);
        }
        // 诊断日志: 打印相机位置 (前 5 次)
        static CAM_DIAG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = CAM_DIAG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n < 5 {
            info!(
                "[DIAG] camera_follow: 相机=({},{},{}) target=({},{})",
                transform.translation.x, transform.translation.y, transform.translation.z,
                target_x, target_y
            );
        }
    }
}

/// 滚轮缩放相机
pub fn camera_zoom_system(
    mut wheel: EventReader<MouseWheel>,
    mut query: Query<&mut OrthographicProjection, With<Camera2d>>,
) {
    let mut zoom_delta = 0.0;
    for event in wheel.read() {
        zoom_delta += event.y;
    }
    if zoom_delta == 0.0 {
        return;
    }
    for mut proj in query.iter_mut() {
        proj.scale *= 1.0 - zoom_delta * 0.1;
        proj.scale = proj.scale.clamp(0.5, 4.0);
    }
}

// ============================================================================
// 死亡处理系统
// ============================================================================

/// 玩家死亡处理: 显示死亡提示，按 R 复活
pub fn death_system(
    mut death_events: EventReader<PlayerDeathEvent>,
    keyboard: Res<ButtonInput<KeyCode>>,
    player: Res<PlayerState>,
    net: Res<NetworkResource>,
    mut death_text: Query<&mut Style, With<DeathOverlay>>,
    mut death_text_inner: Query<&mut Text, With<DeathOverlay>>,
) {
    // 收到死亡事件
    for _ in death_events.read() {
        info!("玩家死亡! 按 R 复活");
    }

    let is_dead = player.logged_in && player.hp <= 0;

    // 更新死亡提示可见性
    for mut style in death_text.iter_mut() {
        style.display = if is_dead {
            Display::Flex
        } else {
            Display::None
        };
    }

    // 更新死亡提示文本
    if is_dead {
        for mut text in death_text_inner.iter_mut() {
            text.sections[0].value = "你已死亡\n按 R 键复活".to_string();
        }

        // 按 R 复活 (发送移动到出生点)
        if keyboard.just_pressed(KeyCode::KeyR) {
            // 发送移动到出生点 (0, 0)
            let payload = crate::codec::encode_move(0.0, 0.0, 0);
            net.send(NetworkCommand::Send {
                msg_id: 3001,
                payload,
            });
            info!("发送复活请求");
        }
    }
}

/// 死亡覆盖层标记
#[derive(Component)]
pub struct DeathOverlay;

// ============================================================================
// 定时查询系统
// ============================================================================

/// 定时查询实体列表 (500ms)
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
