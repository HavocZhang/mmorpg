//! ECS 系统定义
//!
//! 系统是 Bevy ECS 中处理实体和资源的逻辑函数。

use bevy::prelude::*;

use crate::components::*;
use crate::network::NetworkResource;
use crate::resources::*;

// ============================================================================
// 服务器消息处理
// ============================================================================

/// 处理服务器下行消息
pub fn handle_server_message(
    msg_id: u16,
    payload: &[u8],
    player: &mut PlayerState,
    entities: &mut EntityManager,
    other_players: &mut OtherPlayerManager,
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
                } else {
                    debug!(
                        "属性更新: HP={}/{}, MP={}/{}, Exp={}/{}",
                        player.hp, player.max_hp, player.mp, player.max_mp, player.exp, player.max_exp
                    );
                }
            } else if crate::codec::is_json_payload(payload) {
                // JSON fallback (兼容旧服务端)
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) {
                    if let Some(hp) = v.get("hp").and_then(|x| x.as_i64()) {
                        player.hp = hp as i32;
                    }
                    if let Some(max_hp) = v.get("maxHp").and_then(|x| x.as_i64()) {
                        player.max_hp = max_hp as i32;
                    }
                    if let Some(name) = v.get("name").and_then(|x| x.as_str()) {
                        player.name = name.to_string();
                    }
                    if let Some(level) = v.get("level").and_then(|x| x.as_u64()) {
                        player.level = level as u32;
                    }
                    if let Some(x) = v.get("x").and_then(|x| x.as_f64()) {
                        player.x = x as f32;
                    }
                    if let Some(y) = v.get("y").and_then(|y| y.as_f64()) {
                        player.y = y as f32;
                    }
                    player.logged_in = true;
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
                        info!("获得 {} 经验", update.gained);
                    }
                }
            }
        }

        // 6001: 战斗结果
        6001 => {
            if let Some(result) = crate::codec::decode_combat_result(payload) {
                if !result.error.is_empty() {
                    info!("战斗错误: {}", result.error);
                } else if result.swing {
                    info!(
                        "普攻 {} 造成 {} 伤害 (剩余HP: {})",
                        result.target_name, result.damage, result.target_hp
                    );
                } else {
                    info!(
                        "技能攻击 {} 造成 {} 伤害 (暴击: {})",
                        result.target_name, result.damage, result.crit
                    );
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
                info!(
                    "实体 {} ({}) 被击杀",
                    death.entity_id, death.mob_name
                );
                entities.entities.remove(&death.entity_id);
            }
        }

        // 8001: 玩家位置更新 (自己)
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
                info!("玩家进入: {} (UID={})", enter.name, enter.uid);
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
                if let Some(p) = other_players.players.remove(&leave.uid) {
                    info!("玩家离开: {}", p.name);
                }
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
                info!(
                    "实体列表: {} NPC, {} 怪物",
                    list.npcs.len(),
                    list.mobs.len()
                );
            }
        }

        // 9100: 配置数据 (JSON)
        9100 => {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) {
                debug!("收到配置数据");
                if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
                    game_config.items = items.clone();
                }
                if let Some(quests) = v.get("quests").and_then(|x| x.as_array()) {
                    game_config.quests = quests.clone();
                }
                game_config.loaded = true;
            }
        }

        _ => {
            // 未处理的消息 ID
            debug!("未处理的消息 ID: {}", msg_id);
        }
    }
}

// ============================================================================
// 输入/移动系统
// ============================================================================

/// 移动系统: 处理键盘输入，发送移动消息
///
/// WASD 控制方向，100ms 节流避免消息过多
pub fn movement_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    net: Res<NetworkResource>,
    player: Res<PlayerState>,
    mut input: ResMut<InputState>,
) {
    if !player.logged_in || !net.is_connected() {
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

    // 100ms 节流
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
    net.send(crate::network::NetworkCommand::Send {
        msg_id: 3001,
        payload,
    });
}

// ============================================================================
// 渲染系统
// ============================================================================

/// 渲染系统: 同步游戏实体到 Bevy ECS 实体
///
/// - 玩家: 绿色方块
/// - 其他玩家: 蓝色方块
/// - 怪物: 红色方块
/// - NPC: 黄色方块
pub fn render_system(
    player: Res<PlayerState>,
    entities: Res<EntityManager>,
    other_players: Res<OtherPlayerManager>,
    mut commands: Commands,
    player_query: Query<Entity, With<Player>>,
    entity_query: Query<(Entity, &GameEntity)>,
    other_query: Query<(Entity, &OtherPlayer)>,
) {
    // --- 玩家自身 ---
    if let Some(player_entity) = player_query.iter().next() {
        // 更新位置
        commands.entity(player_entity).insert(Transform::from_xyz(
            player.x,
            -player.y,
            10.0,
        ));
    } else if player.logged_in {
        // 首次创建玩家实体 (绿色方块)
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: Color::srgb(0.2, 0.9, 0.2),
                    custom_size: Some(Vec2::new(30.0, 30.0)),
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

    // --- 其他玩家 ---
    // 移除已离开的玩家
    for (entity, other) in other_query.iter() {
        if !other_players.players.contains_key(&other.uid) {
            commands.entity(entity).despawn();
        }
    }
    // 更新或创建
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
            // 蓝色方块
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::srgb(0.2, 0.5, 0.9),
                        custom_size: Some(Vec2::new(25.0, 25.0)),
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
    // 移除已不存在的实体
    for (entity, game_ent) in entity_query.iter() {
        if !entities.entities.contains_key(&game_ent.entity_id) {
            commands.entity(entity).despawn();
        }
    }
    // 更新或创建
    for (eid, info) in &entities.entities {
        let mut found = false;
        for (entity, game_ent) in entity_query.iter() {
            if game_ent.entity_id == *eid {
                commands.entity(entity).insert(Transform::from_xyz(
                    info.x,
                    -info.y,
                    5.0,
                ));
                commands.entity(entity).insert(HealthBar::new(info.hp, info.max_hp));
                found = true;
                break;
            }
        }
        if !found {
            let size = if info.is_npc() { 28.0 } else { 24.0 };
            let color = if info.is_npc() {
                Color::srgb(0.9, 0.8, 0.2) // NPC: 黄色
            } else {
                Color::srgb(0.9, 0.2, 0.2) // 怪物: 红色
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
        transform.translation.x = player.x;
        transform.translation.y = -player.y;
    }
}
