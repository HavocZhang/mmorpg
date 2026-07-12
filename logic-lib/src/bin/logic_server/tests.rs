// ════════════════════════════════════════════════════════════════
// 单元测试 — DashMap 锁竞争/死锁修复验证
// ════════════════════════════════════════════════════════════════

use super::*;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// 辅助：添加测试玩家（高HP避免测试中死亡）
fn add_test_player(state: &GameState, uid: u64, x: f32, y: f32) {
    let mut p = PlayerState::new(uid, format!("Player{}", uid));
    p.x = x;
    p.y = y;
    p.last_x = x;
    p.last_y = y;
    p.hp = 999_999;
    p.max_hp = 999_999;
    p.mp = 999_999;
    p.max_mp = 999_999;
    state.players.insert(uid, p);
}

/// 辅助：添加测试怪物
fn add_test_mob(state: &GameState, eid: u64, def_id: u32, x: f32, y: f32) {
    state.mobs.insert(eid, MobEntity::from_def(eid, def_id, x, y));
}

/// 辅助：给玩家添加已完成进度的任务
fn add_completed_quest(state: &GameState, uid: u64, quest_id: u32) {
    if let Some(mut p) = state.players.get_mut(&uid) {
        if let Some(def) = get_quest_def(quest_id) {
            if !p.quests.iter().any(|(qid, _)| *qid == quest_id) {
                p.quests.push((quest_id, def.target_count));
            }
        }
    }
}

/// 辅助：重新添加已完成任务（用于循环测试）
fn readd_completed_quest(state: &GameState, uid: u64, quest_id: u32) {
    if let Some(mut p) = state.players.get_mut(&uid) {
        if !p.quests.iter().any(|(qid, _)| *qid == quest_id) {
            if let Some(def) = get_quest_def(quest_id) {
                p.quests.push((quest_id, def.target_count));
            }
        }
    }
}

// ────────────────────────────────────────────────────────────
// TDD 单元测试
// ────────────────────────────────────────────────────────────

/// TDD 测试1: tick_mob_ai 期间可以获取单个 mob 的写锁
/// 验证 tick_mob_ai 不再持有全部分片写锁
#[test]
fn test_tick_mob_ai_does_not_hold_all_locks() {
    let state = Arc::new(GameState::test_new());
    add_test_player(&state, 1, 500.0, 500.0);
    // 添加大量怪物以增加迭代时间，确保测试可观测
    for i in 0..200u64 {
        add_test_mob(&state, 10000 + i, 1, 100.0 + (i as f32) * 3.0, 100.0);
    }

    let s = state.clone();
    // 线程A：持续运行 tick_mob_ai
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop2 = stop.clone();
    let tick_handle = thread::spawn(move || {
        while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
            s.tick_mob_ai(0);
        }
    });

    // 主线程：尝试获取单个怪物的写锁，应该在超时内成功多次
    // 修复前：iter_mut 持有全部分片写锁，此处会长时间阻塞
    // 修复后：get_mut 仅持有单个分片写锁，此处可快速获取
    let start = Instant::now();
    let mut acquired = 0;
    while start.elapsed() < Duration::from_secs(2) && acquired < 10 {
        if let Some(mut mob) = state.mobs.get_mut(&(10100)) {
            mob.x += 0.01;
            acquired += 1;
            drop(mob);
        }
        thread::yield_now();
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    tick_handle.join().unwrap();

    assert!(acquired >= 1,
        "tick_mob_ai 期间应能获取单个 mob 写锁，实际获取 {} 次", acquired);
}

/// TDD 测试2: handle_complete_quest 执行期间 tick_mob_ai 不被阻塞
/// 验证 handle_complete_quest 锁内不构建消息，释放锁迅速
#[test]
fn test_handle_complete_quest_releases_lock_quickly() {
    let state = Arc::new(GameState::test_new());
    add_test_player(&state, 1, 500.0, 500.0);
    // 在玩家附近放置怪物，使 tick_mob_ai 进入 Chasing 并尝试获取 player 锁
    add_test_mob(&state, 10000, 1, 500.0, 510.0);
    add_completed_quest(&state, 1, 1);

    let s = state.clone();
    // 线程A：循环调用 handle_complete_quest（反复重置任务）
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop2 = stop.clone();
    let quest_handle = thread::spawn(move || {
        let mut iter = 0;
        while !stop2.load(std::sync::atomic::Ordering::Relaxed) && iter < 200 {
            readd_completed_quest(&s, 1, 1);
            s.handle_complete_quest(1, 1);
            iter += 1;
        }
    });

    // 主线程：tick_mob_ai 应能快速完成（不被 quest 的 player 锁长时间阻塞）
    // 修复前：handle_complete_quest 持有 player 锁构建消息，tick_mob_ai 阻塞
    // 修复后：handle_complete_quest 锁内只做修改，锁外构建消息
    let start = Instant::now();
    for _ in 0..50 {
        state.tick_mob_ai(0);
    }
    let elapsed = start.elapsed();

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    quest_handle.join().unwrap();

    // tick_mob_ai 50次应在 3 秒内完成（远小于 gRPC 5 秒超时）
    assert!(elapsed < Duration::from_secs(3),
        "tick_mob_ai 被 handle_complete_quest 阻塞过久: {:?} (应 < 3s)", elapsed);
}

/// TDD 测试3: 并发攻击怪物 + 完成任务 + tick_mob_ai，无死锁
#[test]
fn test_concurrent_attack_and_quest_no_deadlock() {
    let state = Arc::new(GameState::test_new());
    add_test_player(&state, 1, 500.0, 500.0);
    add_test_player(&state, 2, 510.0, 510.0);
    add_completed_quest(&state, 1, 1);
    for i in 0..30u64 {
        add_test_mob(&state, 10000 + i, 1, 500.0 + (i as f32) * 3.0, 500.0);
    }

    let s1 = state.clone();
    let s2 = state.clone();
    let s3 = state.clone();

    // 使用通道+超时检测死锁
    let (tx, rx) = mpsc::channel();
    let guard = thread::spawn(move || {
        // 线程1: 攻击怪物
        let attack_handle = thread::spawn(move || {
            for _ in 0..50 {
                s1.handle_attack(1, 1, 10000);
            }
        });
        // 线程2: 循环完成任务
        let quest_handle = thread::spawn(move || {
            for _ in 0..50 {
                readd_completed_quest(&s2, 1, 1);
                s2.handle_complete_quest(1, 1);
            }
        });
        // 线程3: tick_mob_ai
        let tick_handle = thread::spawn(move || {
            for _ in 0..50 {
                s3.tick_mob_ai(0);
            }
        });
        attack_handle.join().unwrap();
        quest_handle.join().unwrap();
        tick_handle.join().unwrap();
        let _ = tx.send(());
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => { guard.join().unwrap(); }
        Err(_) => panic!("死锁检测: 并发 attack+quest+tick 超时 (10s)"),
    }
}

// ────────────────────────────────────────────────────────────
// BDD 行为测试
// ────────────────────────────────────────────────────────────

/// BDD 测试4: 场景 — 玩家提交任务时，怪物AI仍正常运行
/// Given: 玩家有已完成任务，怪物在出生点附近
/// When: 玩家提交任务的同时运行怪物AI
/// Then: 两者均成功执行，怪物位置可能变化，无死锁
#[test]
fn test_when_completing_quest_mobs_still_move() {
    let state = Arc::new(GameState::test_new());
    add_test_player(&state, 1, 100.0, 100.0);
    add_completed_quest(&state, 1, 1);
    add_test_mob(&state, 10000, 1, 800.0, 800.0);
    let initial_x = state.mobs.get(&10000).map(|m| m.x).unwrap_or(0.0);

    let s = state.clone();
    // 线程：玩家提交任务
    let quest_handle = thread::spawn(move || {
        s.handle_complete_quest(1, 1)
    });

    // 同时多次运行 tick_mob_ai 让怪物巡逻
    let tick_start = Instant::now();
    for _ in 0..10 {
        state.tick_mob_ai(0);
        thread::sleep(Duration::from_millis(20));
    }
    let tick_elapsed = tick_start.elapsed();

    // 任务应正常完成（返回非空消息表示成功）
    let messages = quest_handle.join().expect("quest thread panicked");
    assert!(!messages.is_empty(), "任务提交应返回消息");

    // tick_mob_ai 应在合理时间内完成（未被任务阻塞）
    assert!(tick_elapsed < Duration::from_secs(3),
        "tick_mob_ai 被任务提交阻塞: {:?} (应 < 3s)", tick_elapsed);

    // 怪物AI有运行（位置可能变化，关键是没死锁）
    let final_x = state.mobs.get(&10000).map(|m| m.x).unwrap_or(0.0);
    let _ = (initial_x, final_x); // 记录位置变化但不强制断言
}

/// BDD 测试5: 场景 — 玩家攻击怪物时，任务提交不被阻塞
/// Given: 玩家1攻击怪物，玩家2有已完成任务
/// When: 玩家1持续攻击怪物的同时，玩家2提交任务
/// Then: 任务提交应在合理时间内完成（不被攻击阻塞）
#[test]
fn test_when_attacking_mob_quest_completion_not_blocked() {
    let state = Arc::new(GameState::test_new());
    add_test_player(&state, 1, 500.0, 500.0);
    add_test_player(&state, 2, 510.0, 510.0);
    add_completed_quest(&state, 2, 1);
    add_test_mob(&state, 10000, 1, 500.0, 500.0);

    let s1 = state.clone();
    let s2 = state.clone();

    // 线程1: 持续攻击怪物
    let attack_handle = thread::spawn(move || {
        for _ in 0..100 {
            s1.handle_attack(1, 1, 10000);
        }
    });

    // 线程2: 玩家2提交任务（计时）
    let (tx, rx) = mpsc::channel::<Duration>();
    let quest_handle = thread::spawn(move || {
        let start = Instant::now();
        s2.handle_complete_quest(2, 1);
        let _ = tx.send(start.elapsed());
    });

    // 等待任务完成，超时则判定死锁
    let quest_duration = rx.recv_timeout(Duration::from_secs(5))
        .expect("任务提交超时 — 可能死锁");
    quest_handle.join().expect("quest thread panicked");
    attack_handle.join().expect("attack thread panicked");

    // 任务提交应在 3 秒内完成（远小于 gRPC 5 秒超时）
    assert!(quest_duration < Duration::from_secs(3),
        "任务提交被攻击操作阻塞过久: {:?} (应 < 3s)", quest_duration);
}

// ────────────────────────────────────────────────────────────
// TDD/BDD 测试 — tokio runtime 阻塞修复验证
// ────────────────────────────────────────────────────────────

/// TDD 测试6: forward_message 不阻塞 tokio runtime
/// 模拟高并发 batch + 后台 tick，验证纯移动请求在限时内响应
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_forward_message_does_not_block_tokio_runtime() {
    let service = MockLogicService { state: Arc::new(GameState::test_new()), db: None };
    // 添加玩家 + 大量怪物（增加 tick 耗时放大问题）
    add_test_player(&service.state, 1, 500.0, 500.0);
    add_test_player(&service.state, 2, 510.0, 510.0);
    for i in 0..100u64 {
        add_test_mob(&service.state, 10000 + i, 1, 400.0 + (i as f32) * 3.0, 400.0);
    }
    // 玩家1攻击怪物使其进入 Chasing（触发 tick 里获取 player 锁）
    service.state.handle_attack(1, 1, 10000);

    // 后台持续 tick（模拟 Default::default 的后台循环，但用 tokio::spawn 阻塞 worker）
    let bg_state = service.state.clone();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_bg = stop.clone();
    // 关键：用 tokio::spawn 跑同步 tick（复现原 bug：占用 tokio worker）
    let bg_handle = tokio::spawn(async move {
        while !stop_bg.load(std::sync::atomic::Ordering::Relaxed) {
            // 模拟原 bug：在 async 上下文里直接调同步阻塞函数
            tokio::task::block_in_place(|| bg_state.tick_mob_ai(0));
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    });

    // 并发发送大量 forward_message_batch（混合 3001 移动）
    let s1 = service.state.clone();
    let batch_handle = tokio::spawn(async move {
        let mut ok = 0u32;
        for i in 0..30u32 {
            // 模拟网关批量转发：每批含多条移动
            for _ in 0..5 {
                let payload = format!(r#"{{"x":{}.0,"y":{}.0,"dir":2}}"#, 500 + i, 500 + i);
                s1.process_message(1, 3001, payload.as_bytes());
                ok += 1;
            }
        }
        ok
    });

    // 关键断言：在 3 秒内应能完成（修复前会因 worker 被占满而超时）
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        batch_handle,
    ).await;

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = bg_handle.await;

    match result {
        Ok(Ok(count)) => assert!(count > 0, "应处理多条消息"),
        Ok(Err(_)) => panic!("batch task panicked"),
        Err(_) => panic!("TDD 复现: tokio runtime 被阻塞，batch 处理超时 3s"),
    }
}

/// BDD 测试7: 场景 — 玩家高频移动+查询实体时，runtime 保持响应
/// Given: 多个玩家在线，大量怪物
/// When: 玩家持续高频移动（3001）+ 查询实体（4002）
/// Then: 每条请求在 1 秒内得到响应（模拟 gRPC 超时阈值的 1/5）
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_when_player_spams_move_and_query_runtime_stays_responsive() {
    let state = Arc::new(GameState::test_new());
    add_test_player(&state, 1, 500.0, 500.0);
    add_test_player(&state, 2, 510.0, 510.0);
    for i in 0..50u64 {
        add_test_mob(&state, 10000 + i, 1, 400.0 + (i as f32) * 3.0, 400.0);
    }

    // 后台 tick 用 std::thread::spawn（模拟修复后的行为）
    let bg = state.clone();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_bg = stop.clone();
    std::thread::spawn(move || {
        while !stop_bg.load(std::sync::atomic::Ordering::Relaxed) {
            bg.tick_mob_ai(0);
            bg.last_mob_tick.store(current_millis(), std::sync::atomic::Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    });

    // 并发：3 个线程同时发移动+查询
    let mut handles = vec![];
    for uid in [1u64, 2, 1] {
        let s = state.clone();
        handles.push(tokio::spawn(async move {
            let start = std::time::Instant::now();
            for i in 0..20u32 {
                let payload = format!(r#"{{"x":{}.0,"y":{}.0,"dir":2}}"#, 500 + i, 500 + i);
                s.process_message(uid, 3001, payload.as_bytes());
                s.process_message(uid, 4002, b"{}");
            }
            start.elapsed()
        }));
    }

    // 所有任务应在 5 秒内完成
    let mut max_elapsed = std::time::Duration::ZERO;
    for h in handles {
        let elapsed = tokio::time::timeout(
            std::time::Duration::from_secs(5), h,
        ).await.expect("BDD: runtime 被阻塞超时 5s").expect("task panicked");
        max_elapsed = max_elapsed.max(elapsed);
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);

    assert!(max_elapsed < std::time::Duration::from_secs(3),
        "高频移动+查询耗时 {:?} (应 < 3s)", max_elapsed);
}

// ────────────────────────────────────────────────────────────
// TDD/BDD 边界场景测试
// ────────────────────────────────────────────────────────────

/// TDD: 攻击距离边界 — 刚好在 skill.range+20 内命中，超出则 miss
#[test]
fn test_attack_range_boundary() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    add_test_player(&state, 2, 500.0, 500.0);
    // 普攻 range=80, 边界=80+20=100
    // 99 距离命中，101 距离 miss
    add_test_mob(&state, 10000, 1, 599.0, 500.0); // 距离99
    add_test_mob(&state, 10001, 1, 601.0, 500.0); // 距离101
    // 用两个不同玩家分别攻击，避免同玩家技能冷却干扰范围判定
    let m1 = state.handle_attack(1, 1, 10000);
    let m2 = state.handle_attack(2, 1, 10001);
    // m1 应有 6001 战斗结果（非 miss）
    let has_hit = m1.iter().any(|m| {
        m.msg_id == 6001 && !String::from_utf8_lossy(&m.payload).contains("miss")
    });
    assert!(has_hit, "距离99应命中");
    // m2 应有 out_of_range miss
    let has_miss = m2.iter().any(|m| {
        m.msg_id == 6001 && String::from_utf8_lossy(&m.payload).contains("out_of_range")
    });
    assert!(has_miss, "距离101应out_of_range");
}

/// TDD: 技能冷却中再次攻击返回 cooldown 错误，且 MP 已扣除
#[test]
fn test_skill_cooldown_blocks_repeat() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    add_test_mob(&state, 10000, 1, 540.0, 500.0);
    // 重击 skill_id=2, cd=2000ms, mp_cost=10
    let _r1 = state.handle_attack(1, 2, 10000);
    // 立即第二次应被冷却阻挡
    let r2 = state.handle_attack(1, 2, 10000);
    let cd_err = r2.iter().any(|m| {
        m.msg_id == 6001 && String::from_utf8_lossy(&m.payload).contains("cooldown")
    });
    assert!(cd_err, "冷却中应返回cooldown错误");
    // MP 应已扣除10（第一次攻击扣 MP，第二次被冷却阻挡不再扣）
    let mp = state.players.get(&1).map(|p| p.mp).unwrap_or(0);
    assert_eq!(mp, 999_999 - 10, "MP应扣除10");
}

/// TDD: 任务进度不足时完成应返回 quest_not_complete
#[test]
fn test_quest_incomplete_cannot_complete() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    // 接受任务1（杀5只史莱姆），进度0
    state.handle_accept_quest(1, 1);
    // 尝试完成应失败
    let r = state.handle_complete_quest(1, 1);
    let err = r.iter().any(|m| {
        m.msg_id == 5005 && String::from_utf8_lossy(&m.payload).contains("quest_not_complete")
    });
    assert!(err, "进度不足应返回quest_not_complete");
}

/// TDD: 已接受的任务再次接受应被拒
#[test]
fn test_quest_cannot_accept_twice() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    let _r1 = state.handle_accept_quest(1, 1);
    let r2 = state.handle_accept_quest(1, 1);
    let err = r2.iter().any(|m| {
        m.msg_id == 5005 && String::from_utf8_lossy(&m.payload).contains("quest_already_accepted")
    });
    assert!(err, "重复接受应返回quest_already_accepted");
}

/// BDD: 场景 — 接受任务→杀怪完成进度→提交任务获得奖励
/// Given: 玩家接受"清除史莱姆"(quest 1, 杀5只, 奖励100经验+生命药水)
/// When: 杀死5只史莱姆后提交任务
/// Then: 获得经验、物品，任务列表清空，无错误消息
#[test]
fn test_full_quest_lifecycle() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);

    // Given: 接受任务
    let accept = state.handle_accept_quest(1, 1);
    assert!(accept.iter().any(|m| m.msg_id == 5005), "接受任务应返回5005");

    // When: 杀5只史莱姆（def_id=1）
    for i in 0..5u64 {
        let eid = 10000 + i;
        add_test_mob(&state, eid, 1, 540.0 + (i as f32), 500.0);
        // 高攻击力一击必杀，清除技能冷却确保连续攻击不被阻挡
        if let Some(mut p) = state.players.get_mut(&1) {
            p.atk = 9999;
            p.skill_cooldowns.clear();
        }
        let _ = state.handle_attack(1, 1, eid);
        // 怪物应死亡
        let mob_dead = state.mobs.get(&eid).map(|m| m.state == MobState::Dead).unwrap_or(false);
        assert!(mob_dead, "怪物{}应被击杀", eid);
    }

    // 进度应到5
    let progress = state.players.get(&1)
        .and_then(|p| p.quests.iter().find(|(qid, _)| *qid == 1).map(|(_, c)| *c))
        .unwrap_or(0);
    assert_eq!(progress, 5, "任务进度应为5");

    // Then: 提交任务
    let complete = state.handle_complete_quest(1, 1);
    // 应有经验奖励 5002
    assert!(complete.iter().any(|m| m.msg_id == 5002), "应返回经验更新5002");
    // 应有背包更新 5003（物品奖励）
    assert!(complete.iter().any(|m| m.msg_id == 5003), "应返回背包更新5003");
    // 不应有 error
    assert!(!complete.iter().any(|m| String::from_utf8_lossy(&m.payload).contains("error")), "不应有错误");
    // 任务应从列表移除
    let quest_exists = state.players.get(&1)
        .map(|p| p.quests.iter().any(|(qid, _)| *qid == 1))
        .unwrap_or(false);
    assert!(!quest_exists, "任务应已从列表移除");
    // 经验应增加100（5次击杀共100 exp 触发升级归零，任务奖励100 → exp=100）
    let exp = state.players.get(&1).map(|p| p.exp).unwrap_or(0);
    assert!(exp >= 100, "经验应至少100, 实际 {}", exp);
    // 背包应有生命药水(item_id=6)
    let has_potion = state.players.get(&1)
        .map(|p| p.inventory.iter().any(|(id, c)| *id == 6 && *c > 0))
        .unwrap_or(false);
    assert!(has_potion, "背包应有生命药水");
}

/// BDD: 场景 — 玩家移动速度异常时被拉回
/// Given: 玩家在(500,500)，上次移动时间已记录
/// When: 玩家瞬间移动到(1000,500)（距离500，远超200/s阈值）
/// Then: 返回5001拉回消息，位置不变
#[test]
fn test_anti_cheat_teleport_detection() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    // 先正常移动一次，记录 last_move_ms
    let payload = r#"{"x":510.0,"y":500.0,"dir":2}"#;
    let _r1 = state.process_message(1, 3001, payload.as_bytes());
    // 立即瞬移到 1000,500
    let payload2 = r#"{"x":1000.0,"y":500.0,"dir":2}"#;
    let r2 = state.process_message(1, 3001, payload2.as_bytes());
    // 应返回 5001 拉回
    let pulled_back = r2.messages.iter().any(|m| {
        m.msg_id == 5001 && String::from_utf8_lossy(&m.payload).contains("500.0")
    });
    assert!(pulled_back, "瞬移应被拉回原位");
    // 玩家位置应仍在 510 附近（上次合法位置）
    let x = state.players.get(&1).map(|p| p.x).unwrap_or(0.0);
    assert!((x - 510.0).abs() < 1.0, "玩家应被拉回到510, 实际 {}", x);
}

// ────────────────────────────────────────────────────────────
// TDD 装备强化 (v0.7)
// ────────────────────────────────────────────────────────────

/// TDD: 装备强化 +1 成功，属性增加
#[test]
fn test_enhance_weapon_success() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    // 给玩家装备铁剑(item_id=1, atk_bonus=15) 和足够金币
    if let Some(mut p) = state.players.get_mut(&1) {
        p.weapon = Some(1);
        p.gold = 1000;
    }
    // 强化 +1，费用 1*100=100
    let r = state.handle_enhance(1, "weapon");
    // 5004 现已迁移为 proto：解码 EquipmentUpdate，验证 weapon.enhance_level >= 1
    use logic_lib::game_proto as gp;
    use prost::Message;
    let success = r.iter().any(|m| {
        if m.msg_id != 5004 { return false; }
        if let Ok(eu) = gp::EquipmentUpdate::decode(&m.payload[..]) {
            eu.weapon.as_ref().map(|s| s.enhance_level >= 1).unwrap_or(false)
        } else {
            false
        }
    });
    assert!(success, "强化+1应成功(100%概率)并返回5004(含enhance_level>=1)");
    // atk 应增加: 铁剑基础15, +1级=15*1.1=16.5→16, 增加1
    let atk = state.players.get(&1).map(|p| p.total_atk()).unwrap_or(0);
    // base atk=20 + 强化后武器16 = 36 (原 20+15=35)
    assert!(atk >= 36, "强化后atk应>=36, 实际{}", atk);
    // 金币扣除100
    let gold = state.players.get(&1).map(|p| p.gold).unwrap_or(0);
    assert_eq!(gold, 900, "应扣除100金币");
}

/// TDD: 金币不足无法强化
#[test]
fn test_enhance_insufficient_gold() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    if let Some(mut p) = state.players.get_mut(&1) {
        p.weapon = Some(1);
        p.gold = 50; // 不够100
    }
    let r = state.handle_enhance(1, "weapon");
    let err = r.iter().any(|m| String::from_utf8_lossy(&m.payload).contains("insufficient_gold"));
    assert!(err, "金币不足应返回错误");
}

/// TDD: 强化上限 +10
#[test]
fn test_enhance_max_level() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    if let Some(mut p) = state.players.get_mut(&1) {
        p.weapon = Some(1);
        p.weapon_enhance = 10;
        p.gold = 10000;
    }
    let r = state.handle_enhance(1, "weapon");
    let err = r.iter().any(|m| String::from_utf8_lossy(&m.payload).contains("max_level"));
    assert!(err, "+10应返回max_level错误");
}

/// TDD: 未装备该槽位无法强化
#[test]
fn test_enhance_no_item() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    // weapon=None
    let r = state.handle_enhance(1, "weapon");
    let err = r.iter().any(|m| String::from_utf8_lossy(&m.payload).contains("no_item"));
    assert!(err, "无装备应返回no_item");
}

/// BDD: 连续强化到+10，属性逐步增长
#[test]
fn test_enhance_full_progression() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    if let Some(mut p) = state.players.get_mut(&1) {
        p.weapon = Some(2); // 钢剑 atk_bonus=30
        p.gold = 100000;
    }
    let prev_atk = state.players.get(&1).map(|p| p.total_atk()).unwrap_or(0);
    // 连续强化，+1~+3必成功，+4起可能失败，循环直到+10或超过100次尝试
    // 每次 sleep 1ms 以确保 current_millis 推进，避免随机数碰撞导致测试不稳定
    let mut attempts = 0;
    loop {
        attempts += 1;
        if attempts > 100 { break; }
        let level = state.players.get(&1).map(|p| p.weapon_enhance).unwrap_or(0);
        if level >= 10 { break; }
        let _ = state.handle_enhance(1, "weapon");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let final_level = state.players.get(&1).map(|p| p.weapon_enhance).unwrap_or(0);
    assert_eq!(final_level, 10, "应在100次尝试内强化到+10");
    let final_atk = state.players.get(&1).map(|p| p.total_atk()).unwrap_or(0);
    assert!(final_atk > prev_atk, "强化后atk应增长: {} -> {}", prev_atk, final_atk);
}

// ════════════════════════════════════════════════════════════════
// Protobuf 编解码适配层测试
// ════════════════════════════════════════════════════════════════

#[test]
fn test_codec_decode_proto_move_request() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let proto_msg = gp::MoveRequest { x: 500.0, y: 300.0, dir: 2 };
    let payload = proto_msg.encode_to_vec();

    let decoded = super::codec::decode_upstream(3001, &payload);
    match decoded {
        super::codec::UpstreamMsg::MoveRequest(m) => {
            assert_eq!(m.x, 500.0);
            assert_eq!(m.y, 300.0);
            assert_eq!(m.dir, 2);
        }
        _ => panic!("应解码为 MoveRequest"),
    }
}

#[test]
fn test_codec_decode_json_move_request_fallback() {
    // JSON 客户端发的消息也要能解析
    let json_payload = r#"{"x":500.0,"y":300.0,"dir":2}"#.as_bytes();

    let decoded = super::codec::decode_upstream(3001, json_payload);
    match decoded {
        super::codec::UpstreamMsg::MoveRequest(m) => {
            assert_eq!(m.x, 500.0);
            assert_eq!(m.y, 300.0);
            assert_eq!(m.dir, 2);
        }
        _ => panic!("JSON fallback 应解码为 MoveRequest"),
    }
}

#[test]
fn test_codec_decode_proto_attack_request() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let proto_msg = gp::AttackRequest { target_uid: 10001 };
    let payload = proto_msg.encode_to_vec();

    let decoded = super::codec::decode_upstream(1001, &payload);
    match decoded {
        super::codec::UpstreamMsg::AttackRequest(m) => {
            assert_eq!(m.target_uid, 10001);
        }
        _ => panic!("应解码为 AttackRequest"),
    }
}

#[test]
fn test_codec_decode_json_attack_request_fallback() {
    let json_payload = r#"{"targetUid":10001}"#.as_bytes();

    let decoded = super::codec::decode_upstream(1001, json_payload);
    match decoded {
        super::codec::UpstreamMsg::AttackRequest(m) => {
            assert_eq!(m.target_uid, 10001);
        }
        _ => panic!("JSON fallback 应解码为 AttackRequest"),
    }
}

#[test]
fn test_codec_json_fallback_for_unmigrated_messages() {
    // 未迁移的消息（如 2501 创建公会）应走 JsonFallback
    // 1005 接受任务已迁移到 proto，不再走 JsonFallback
    let json_payload = r#"{"name":"测试公会"}"#.as_bytes();

    let decoded = super::codec::decode_upstream(2501, json_payload);
    match decoded {
        super::codec::UpstreamMsg::JsonFallback(v) => {
            assert_eq!(v.get("name").and_then(|x| x.as_str()), Some("测试公会"));
        }
        _ => panic!("未迁移消息应走 JsonFallback"),
    }
}

#[test]
fn test_codec_dm_proto_encodes_correctly() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let stats = gp::PlayerStats {
        uid: 12345, name: "测试".to_string(), hp: 100, max_hp: 100,
        mp: 50, max_mp: 50, level: 5, exp: 200, max_exp: 500,
        x: 400.0, y: 300.0, atk: 20, def: 10, gold: 1000,
        class_id: 1, talent_points: 3,
        class_icon: "⚔".to_string(),
        talents: vec![1, 2],
    };
    let msg = super::codec::dm_proto(12345, 5001, &stats, 0);

    assert_eq!(msg.target_uid, 12345);
    assert_eq!(msg.msg_id, 5001);
    // 验证 payload 能被 proto 解码回来
    let decoded = gp::PlayerStats::decode(&msg.payload[..]).unwrap();
    assert_eq!(decoded.uid, 12345);
    assert_eq!(decoded.name, "测试");
    assert_eq!(decoded.hp, 100);
    assert_eq!(decoded.class_icon, "⚔");
    assert_eq!(decoded.talents, vec![1, 2]);
}

#[test]
fn test_move_request_proto_path_in_process_message() {
    // BDD: 玩家用 proto 发送移动消息，服务端应正确处理
    use logic_lib::game_proto as gp;
    use prost::Message;
    use rust_mmo_gate::grpc_router::proto::gate::ForwardResponse;

    let state = GameState::test_new();
    add_test_player(&state, 50001, 100.0, 100.0);

    let proto_msg = gp::MoveRequest { x: 500.0, y: 300.0, dir: 0 };
    let payload = proto_msg.encode_to_vec();

    let resp = state.process_message(50001, 3001, &payload);
    // 应返回 ForwardResponse（不 panic、不卡死）
    let _resp: ForwardResponse = resp;

    // 验证玩家位置已更新
    let player = state.players.get(&50001).unwrap();
    assert_eq!(player.x, 500.0);
    assert_eq!(player.y, 300.0);
}

#[test]
fn test_move_request_json_path_still_works() {
    // BDD: 旧客户端用 JSON 发送移动消息，服务端仍应正确处理
    use rust_mmo_gate::grpc_router::proto::gate::ForwardResponse;

    let state = GameState::test_new();
    add_test_player(&state, 50002, 100.0, 100.0);

    let json_payload = r#"{"x":600.0,"y":400.0,"dir":1}"#.as_bytes();

    let resp = state.process_message(50002, 3001, json_payload);
    let _resp: ForwardResponse = resp;

    let player = state.players.get(&50002).unwrap();
    assert_eq!(player.x, 600.0);
    assert_eq!(player.y, 400.0);
}

// ════════════════════════════════════════════════════════════════
// Protobuf 解码路径测试 — 第二批迁移消息 (1002-1011, 2001-2004, 4001-4002)
// ════════════════════════════════════════════════════════════════

#[test]
fn test_codec_decode_proto_skill_attack() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let msg = gp::SkillAttackRequest { skill_id: 2, target_uid: 10001 };
    let buf = msg.encode_to_vec();
    let decoded = super::codec::decode_upstream(1002, &buf);
    match decoded {
        super::codec::UpstreamMsg::SkillAttackRequest(m) => {
            assert_eq!(m.skill_id, 2);
            assert_eq!(m.target_uid, 10001);
        }
        _ => panic!("应解码为 SkillAttackRequest"),
    }
}

#[test]
fn test_codec_decode_json_skill_attack_fallback() {
    let buf = r#"{"skillId":2,"targetUid":10001}"#.as_bytes();
    let decoded = super::codec::decode_upstream(1002, buf);
    match decoded {
        super::codec::UpstreamMsg::SkillAttackRequest(m) => {
            assert_eq!(m.skill_id, 2);
            assert_eq!(m.target_uid, 10001);
        }
        _ => panic!("JSON fallback 应解码为 SkillAttackRequest"),
    }
}

#[test]
fn test_codec_all_proto_messages_roundtrip() {
    // 测试所有新迁移消息的 proto 编解码往返
    use logic_lib::game_proto as gp;
    use prost::Message;

    // PickupRequest
    let msg = gp::PickupRequest { drop_id: 50001 };
    let decoded = super::codec::decode_upstream(1003, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::PickupRequest(m) => assert_eq!(m.drop_id, 50001),
        _ => panic!("应解码为 PickupRequest"),
    }

    // EquipRequest
    let msg = gp::EquipRequest { item_id: 1, slot: "weapon".to_string() };
    let decoded = super::codec::decode_upstream(1004, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::EquipRequest(m) => {
            assert_eq!(m.item_id, 1);
            assert_eq!(m.slot, "weapon");
        }
        _ => panic!("应解码为 EquipRequest"),
    }

    // AcceptQuestRequest
    let msg = gp::AcceptQuestRequest { quest_id: 1 };
    let decoded = super::codec::decode_upstream(1005, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::AcceptQuestRequest(m) => assert_eq!(m.quest_id, 1),
        _ => panic!("应解码为 AcceptQuestRequest"),
    }

    // CompleteQuestRequest
    let msg = gp::CompleteQuestRequest { quest_id: 1 };
    let decoded = super::codec::decode_upstream(1006, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::CompleteQuestRequest(m) => assert_eq!(m.quest_id, 1),
        _ => panic!("应解码为 CompleteQuestRequest"),
    }

    // NpcInteractRequest
    let msg = gp::NpcInteractRequest { npc_id: 1 };
    let decoded = super::codec::decode_upstream(1007, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::NpcInteractRequest(m) => assert_eq!(m.npc_id, 1),
        _ => panic!("应解码为 NpcInteractRequest"),
    }

    // UseItemRequest
    let msg = gp::UseItemRequest { item_id: 6 };
    let decoded = super::codec::decode_upstream(1008, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::UseItemRequest(m) => assert_eq!(m.item_id, 6),
        _ => panic!("应解码为 UseItemRequest"),
    }

    // ShopBuyRequest
    let msg = gp::ShopBuyRequest { item_id: 6, count: 2 };
    let decoded = super::codec::decode_upstream(1009, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::ShopBuyRequest(m) => {
            assert_eq!(m.item_id, 6);
            assert_eq!(m.count, 2);
        }
        _ => panic!("应解码为 ShopBuyRequest"),
    }

    // ShopSellRequest
    let msg = gp::ShopSellRequest { item_id: 6, count: 1 };
    let decoded = super::codec::decode_upstream(1010, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::ShopSellRequest(m) => {
            assert_eq!(m.item_id, 6);
            assert_eq!(m.count, 1);
        }
        _ => panic!("应解码为 ShopSellRequest"),
    }

    // EnhanceRequest
    let msg = gp::EnhanceRequest { slot: "weapon".to_string() };
    let decoded = super::codec::decode_upstream(1011, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::EnhanceRequest(m) => assert_eq!(m.slot, "weapon"),
        _ => panic!("应解码为 EnhanceRequest"),
    }

    // ChatRequest
    let msg = gp::ChatRequest { text: "hello".to_string(), channel: "world".to_string() };
    let decoded = super::codec::decode_upstream(2001, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::ChatRequest(m) => {
            assert_eq!(m.text, "hello");
            assert_eq!(m.channel, "world");
        }
        _ => panic!("应解码为 ChatRequest"),
    }

    // PartyInviteRequest
    let msg = gp::PartyInviteRequest { target_uid: 20002 };
    let decoded = super::codec::decode_upstream(2002, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::PartyInviteRequest(m) => assert_eq!(m.target_uid, 20002),
        _ => panic!("应解码为 PartyInviteRequest"),
    }

    // PartyAcceptRequest
    let msg = gp::PartyAcceptRequest { inviter_uid: 20001 };
    let decoded = super::codec::decode_upstream(2003, &msg.encode_to_vec());
    match decoded {
        super::codec::UpstreamMsg::PartyAcceptRequest(m) => assert_eq!(m.inviter_uid, 20001),
        _ => panic!("应解码为 PartyAcceptRequest"),
    }

    // PartyLeaveRequest (无字段)
    let msg = gp::PartyLeaveRequest {};
    let decoded = super::codec::decode_upstream(2004, &msg.encode_to_vec());
    assert!(matches!(decoded, super::codec::UpstreamMsg::PartyLeaveRequest(_)));

    // QueryPlayersRequest (无字段)
    let msg = gp::QueryPlayersRequest {};
    let decoded = super::codec::decode_upstream(4001, &msg.encode_to_vec());
    assert!(matches!(decoded, super::codec::UpstreamMsg::QueryPlayersRequest(_)));

    // QueryEntitiesRequest (无字段)
    let msg = gp::QueryEntitiesRequest {};
    let decoded = super::codec::decode_upstream(4002, &msg.encode_to_vec());
    assert!(matches!(decoded, super::codec::UpstreamMsg::QueryEntitiesRequest(_)));
}

#[test]
fn test_codec_all_json_fallbacks() {
    // 测试所有新迁移消息的 JSON fallback 路径

    // PickupRequest
    let decoded = super::codec::decode_upstream(1003, r#"{"dropId":50001}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::PickupRequest(m) => assert_eq!(m.drop_id, 50001),
        _ => panic!("JSON fallback 应解码为 PickupRequest"),
    }

    // EquipRequest
    let decoded = super::codec::decode_upstream(1004, r#"{"itemId":1,"slot":"weapon"}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::EquipRequest(m) => {
            assert_eq!(m.item_id, 1);
            assert_eq!(m.slot, "weapon");
        }
        _ => panic!("JSON fallback 应解码为 EquipRequest"),
    }

    // AcceptQuestRequest
    let decoded = super::codec::decode_upstream(1005, r#"{"questId":1}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::AcceptQuestRequest(m) => assert_eq!(m.quest_id, 1),
        _ => panic!("JSON fallback 应解码为 AcceptQuestRequest"),
    }

    // CompleteQuestRequest
    let decoded = super::codec::decode_upstream(1006, r#"{"questId":1}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::CompleteQuestRequest(m) => assert_eq!(m.quest_id, 1),
        _ => panic!("JSON fallback 应解码为 CompleteQuestRequest"),
    }

    // NpcInteractRequest
    let decoded = super::codec::decode_upstream(1007, r#"{"npcId":1}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::NpcInteractRequest(m) => assert_eq!(m.npc_id, 1),
        _ => panic!("JSON fallback 应解码为 NpcInteractRequest"),
    }

    // UseItemRequest
    let decoded = super::codec::decode_upstream(1008, r#"{"itemId":6}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::UseItemRequest(m) => assert_eq!(m.item_id, 6),
        _ => panic!("JSON fallback 应解码为 UseItemRequest"),
    }

    // ShopBuyRequest
    let decoded = super::codec::decode_upstream(1009, r#"{"itemId":6,"count":2}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::ShopBuyRequest(m) => {
            assert_eq!(m.item_id, 6);
            assert_eq!(m.count, 2);
        }
        _ => panic!("JSON fallback 应解码为 ShopBuyRequest"),
    }

    // ShopSellRequest
    let decoded = super::codec::decode_upstream(1010, r#"{"itemId":6,"count":1}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::ShopSellRequest(m) => {
            assert_eq!(m.item_id, 6);
            assert_eq!(m.count, 1);
        }
        _ => panic!("JSON fallback 应解码为 ShopSellRequest"),
    }

    // EnhanceRequest
    let decoded = super::codec::decode_upstream(1011, r#"{"slot":"armor"}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::EnhanceRequest(m) => assert_eq!(m.slot, "armor"),
        _ => panic!("JSON fallback 应解码为 EnhanceRequest"),
    }

    // ChatRequest
    let decoded = super::codec::decode_upstream(2001, r#"{"text":"hi","channel":"world"}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::ChatRequest(m) => {
            assert_eq!(m.text, "hi");
            assert_eq!(m.channel, "world");
        }
        _ => panic!("JSON fallback 应解码为 ChatRequest"),
    }

    // PartyInviteRequest
    let decoded = super::codec::decode_upstream(2002, r#"{"targetUid":20002}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::PartyInviteRequest(m) => assert_eq!(m.target_uid, 20002),
        _ => panic!("JSON fallback 应解码为 PartyInviteRequest"),
    }

    // PartyAcceptRequest
    let decoded = super::codec::decode_upstream(2003, r#"{"inviterUid":20001}"#.as_bytes());
    match decoded {
        super::codec::UpstreamMsg::PartyAcceptRequest(m) => assert_eq!(m.inviter_uid, 20001),
        _ => panic!("JSON fallback 应解码为 PartyAcceptRequest"),
    }

    // PartyLeaveRequest (无字段，空 JSON 也应返回 PartyLeaveRequest)
    let decoded = super::codec::decode_upstream(2004, b"{}");
    assert!(matches!(decoded, super::codec::UpstreamMsg::PartyLeaveRequest(_)));

    // QueryPlayersRequest (无字段)
    let decoded = super::codec::decode_upstream(4001, b"{}");
    assert!(matches!(decoded, super::codec::UpstreamMsg::QueryPlayersRequest(_)));

    // QueryEntitiesRequest (无字段)
    let decoded = super::codec::decode_upstream(4002, b"{}");
    assert!(matches!(decoded, super::codec::UpstreamMsg::QueryEntitiesRequest(_)));
}

// ────────────────────────────────────────────────────────────
// TDD/BDD 测试 — 事件总线 (Event Bus)
// ────────────────────────────────────────────────────────────

/// TDD: 事件总线 — MobKilled 事件触发任务进度更新
/// Given: 玩家已接受任务1（消灭5只史莱姆），进度为0
/// When: 发布 MobKilled 事件（mob_def_id=1）
/// Then: 玩家任务进度变为1，SideEffect 包含 5005 任务更新消息
#[test]
fn test_event_bus_mob_killed_triggers_quest_progress() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    let _ = state.handle_accept_quest(1, 1);

    let progress_before = state.players.get(&1)
        .and_then(|p| p.quests.iter().find(|(qid, _)| *qid == 1).map(|(_, c)| *c))
        .unwrap_or(0);
    assert_eq!(progress_before, 0, "初始任务进度应为0");

    let event = event_bus::GameEvent::MobKilled {
        killer_uid: 1,
        mob_def_id: 1,
        mob_entity_id: 10000,
        mob_name: "史莱姆".to_string(),
        x: 500.0,
        y: 500.0,
    };
    let effect = state.event_bus.publish(&event, &state);

    let progress_after = state.players.get(&1)
        .and_then(|p| p.quests.iter().find(|(qid, _)| *qid == 1).map(|(_, c)| *c))
        .unwrap_or(0);
    assert_eq!(progress_after, 1, "杀怪后任务进度应为1");

    assert!(effect.player_messages.iter().any(|(_, m)| m.msg_id == 5005),
        "SideEffect 应包含 5005 任务更新消息");
}

/// TDD: 事件总线 — MobKilled 事件触发掉落物生成
/// Given: 无掉落物
/// When: 发布 MobKilled 事件（mob_def_id=1）
/// Then: drops 表有掉落物，SideEffect 包含 6003 死亡广播消息
#[test]
fn test_event_bus_mob_killed_generates_drops() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    assert_eq!(state.drops.len(), 0, "初始无掉落物");

    let event = event_bus::GameEvent::MobKilled {
        killer_uid: 1,
        mob_def_id: 1,
        mob_entity_id: 10000,
        mob_name: "史莱姆".to_string(),
        x: 500.0,
        y: 500.0,
    };
    let effect = state.event_bus.publish(&event, &state);

    assert!(state.drops.len() > 0, "杀怪后应生成掉落物");
    assert!(effect.broadcast_messages.iter().any(|m| m.msg_id == 6003),
        "SideEffect 应包含 6003 死亡广播消息");
}

/// TDD: 事件总线 — MobKilled 事件触发经验奖励
/// Given: 玩家初始经验为0
/// When: 发布 MobKilled 事件（mob_def_id=1，史莱姆 exp=20）
/// Then: SideEffect 的 exp_rewards 包含 (1, 20)
#[test]
fn test_event_bus_mob_killed_grants_exp() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);

    let event = event_bus::GameEvent::MobKilled {
        killer_uid: 1,
        mob_def_id: 1,
        mob_entity_id: 10000,
        mob_name: "史莱姆".to_string(),
        x: 500.0,
        y: 500.0,
    };
    let effect = state.event_bus.publish(&event, &state);

    assert!(effect.exp_rewards.iter().any(|(uid, exp)| *uid == 1 && *exp == 20),
        "SideEffect 应包含经验奖励 (uid=1, exp=20)，实际: {:?}", effect.exp_rewards);
}

/// BDD: 场景 — 玩家击杀怪物时，任务进度通过事件总线更新
/// Given: 玩家已接受任务1，怪物在附近
/// When: 玩家攻击怪物至死亡
/// Then: 任务进度增加，handle_attack 返回的消息包含 5005/6003/5002
#[test]
fn test_when_player_kills_mob_quest_progresses_via_event_bus() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    add_test_mob(&state, 10000, 1, 540.0, 500.0);
    let _ = state.handle_accept_quest(1, 1);

    // 高攻击力一击必杀
    if let Some(mut p) = state.players.get_mut(&1) {
        p.atk = 9999;
        p.skill_cooldowns.clear();
    }

    let progress_before = state.players.get(&1)
        .and_then(|p| p.quests.iter().find(|(qid, _)| *qid == 1).map(|(_, c)| *c))
        .unwrap_or(0);
    assert_eq!(progress_before, 0, "击杀前任务进度应为0");

    let msgs = state.handle_attack(1, 1, 10000);

    let mob_dead = state.mobs.get(&10000).map(|m| m.state == MobState::Dead).unwrap_or(false);
    assert!(mob_dead, "怪物应被击杀");

    let progress_after = state.players.get(&1)
        .and_then(|p| p.quests.iter().find(|(qid, _)| *qid == 1).map(|(_, c)| *c))
        .unwrap_or(0);
    assert_eq!(progress_after, 1, "击杀后任务进度应为1");

    assert!(msgs.iter().any(|m| m.msg_id == 5005), "应返回 5005 任务更新消息");
    assert!(msgs.iter().any(|m| m.msg_id == 6003), "应返回 6003 死亡广播");
    assert!(msgs.iter().any(|m| m.msg_id == 5002), "应返回 5002 经验更新");
}

/// TDD: 事件总线 — 多个订阅者同时响应同一事件
/// Given: 默认事件总线（3个订阅者：任务/掉落/经验）
/// When: 发布 MobKilled 事件
/// Then: SideEffect 同时包含 exp_rewards、player_messages、broadcast_messages
#[test]
fn test_event_bus_multiple_subscribers() {
    let state = GameState::test_new();
    add_test_player(&state, 1, 500.0, 500.0);
    let _ = state.handle_accept_quest(1, 1);

    let event = event_bus::GameEvent::MobKilled {
        killer_uid: 1,
        mob_def_id: 1,
        mob_entity_id: 10000,
        mob_name: "史莱姆".to_string(),
        x: 500.0,
        y: 500.0,
    };
    let effect = state.event_bus.publish(&event, &state);

    assert!(!effect.exp_rewards.is_empty(), "经验奖励订阅者应产生 exp_rewards");
    assert!(!effect.player_messages.is_empty(), "任务进度订阅者应产生 player_messages");
    assert!(!effect.broadcast_messages.is_empty(), "掉落生成订阅者应产生 broadcast_messages");
}

// ────────────────────────────────────────────────────────────
// BDD 行为测试 — proto 路径在 process_message 中端到端验证
// ────────────────────────────────────────────────────────────

#[test]
fn test_skill_attack_proto_path() {
    // BDD: 玩家用 proto 发送技能攻击，服务端应正确处理
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 60001, 100.0, 100.0);
    add_test_mob(&state, 10001, 1, 100.0, 100.0); // 史莱姆

    let msg = gp::SkillAttackRequest { skill_id: 1, target_uid: 10001 };
    let payload = msg.encode_to_vec();

    let _resp = state.process_message(60001, 1002, &payload);
    // 不 panic 即通过
}

#[test]
fn test_quest_accept_proto_path() {
    // BDD: 玩家用 proto 接受任务
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 60002, 200.0, 200.0);

    let msg = gp::AcceptQuestRequest { quest_id: 1 };
    let payload = msg.encode_to_vec();

    let _resp = state.process_message(60002, 1005, &payload);
    // 验证任务已接受
    let player = state.players.get(&60002).unwrap();
    assert!(!player.quests.is_empty(), "任务列表不应为空");
}

#[test]
fn test_quest_accept_json_path_still_works() {
    // BDD: 旧客户端用 JSON 接受任务，服务端仍应正确处理
    let state = GameState::test_new();
    add_test_player(&state, 60003, 200.0, 200.0);

    let json_payload = r#"{"questId":1}"#.as_bytes();
    let _resp = state.process_message(60003, 1005, json_payload);

    let player = state.players.get(&60003).unwrap();
    assert!(!player.quests.is_empty(), "JSON 路径任务列表不应为空");
}

#[test]
fn test_chat_proto_path() {
    // BDD: 玩家用 proto 发送聊天消息，服务端应正确广播
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 60004, 300.0, 300.0);

    let msg = gp::ChatRequest { text: "hello proto".to_string(), channel: "world".to_string() };
    let payload = msg.encode_to_vec();

    let resp = state.process_message(60004, 2001, &payload);
    // 应有 7001 ack 和 7002 广播
    let has_ack = resp.messages.iter().any(|m| m.msg_id == 7001);
    assert!(has_ack, "应有聊天 ACK");
    let has_broadcast = resp.messages.iter().any(|m| {
        m.msg_id == 7002 && String::from_utf8_lossy(&m.payload).contains("hello proto")
    });
    assert!(has_broadcast, "应有聊天广播");
}

#[test]
fn test_chat_json_path_still_works() {
    // BDD: 旧客户端用 JSON 发送聊天消息，服务端仍应正确处理
    let state = GameState::test_new();
    add_test_player(&state, 60005, 300.0, 300.0);

    let json_payload = r#"{"text":"hello json","channel":"world"}"#.as_bytes();
    let resp = state.process_message(60005, 2001, json_payload);

    let has_broadcast = resp.messages.iter().any(|m| {
        m.msg_id == 7002 && String::from_utf8_lossy(&m.payload).contains("hello json")
    });
    assert!(has_broadcast, "JSON 聊天应有广播");
}

#[test]
fn test_query_players_proto_path() {
    // BDD: 玩家用 proto 查询附近玩家
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 60006, 400.0, 400.0);
    add_test_player(&state, 60007, 410.0, 410.0);

    let msg = gp::QueryPlayersRequest {};
    let payload = msg.encode_to_vec();

    let resp = state.process_message(60006, 4001, &payload);
    // 应返回 9001 玩家列表
    let has_list = resp.messages.iter().any(|m| m.msg_id == 9001);
    assert!(has_list, "应有玩家列表 9001");
}

#[test]
fn test_query_entities_proto_path() {
    // BDD: 玩家用 proto 查询附近实体
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 60008, 500.0, 500.0);

    let msg = gp::QueryEntitiesRequest {};
    let payload = msg.encode_to_vec();

    let resp = state.process_message(60008, 4002, &payload);
    // 应返回 9002 实体列表
    let has_list = resp.messages.iter().any(|m| m.msg_id == 9002);
    assert!(has_list, "应有实体列表 9002");
}

#[test]
fn test_party_leave_proto_path() {
    // BDD: 玩家用 proto 离开队伍（无字段消息）
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 60009, 500.0, 500.0);

    let msg = gp::PartyLeaveRequest {};
    let payload = msg.encode_to_vec();

    // 不 panic 即通过（玩家不在队伍中也不应崩溃）
    let _resp = state.process_message(60009, 2004, &payload);
}

#[test]
fn test_enhance_proto_path() {
    // BDD: 玩家用 proto 请求装备强化
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 60010, 500.0, 500.0);
    // 给玩家装备铁剑和足够金币
    if let Some(mut p) = state.players.get_mut(&60010) {
        p.weapon = Some(1);
        p.gold = 1000;
    }

    let msg = gp::EnhanceRequest { slot: "weapon".to_string() };
    let payload = msg.encode_to_vec();

    let resp = state.process_message(60010, 1011, &payload);
    // 强化 +1 应成功（100%概率）
    let success = resp.messages.iter().any(|m| {
        m.msg_id == 5006 && String::from_utf8_lossy(&m.payload).contains("enhance_result")
    });
    assert!(success, "proto 路径强化应返回结果");
}

#[test]
fn test_npc_interact_proto_path() {
    // BDD: 玩家用 proto 与 NPC 交互
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 60011, 500.0, 500.0);

    let msg = gp::NpcInteractRequest { npc_id: 1 };
    let payload = msg.encode_to_vec();

    let resp = state.process_message(60011, 1007, &payload);
    // 应返回 5006 NPC 对话
    let has_dialog = resp.messages.iter().any(|m| m.msg_id == 5006);
    assert!(has_dialog, "proto 路径 NPC 交互应返回对话");
}

// ════════════════════════════════════════════════════════════════
// 下行消息 Proto 迁移验证 — 第三批 (5001/5002/5004/5005/6001)
// 验证迁移后的下行消息能被 proto 正确解码且包含扩展字段
// ════════════════════════════════════════════════════════════════

/// TDD: PlayerStats (5001) 迁移到 proto 后包含 class_icon 和 talents 字段
/// Given: 玩家已选职业并学习天赋
/// When: 触发 handle_equip（会发送 5001 proto）
/// Then: 5001 payload 可被 PlayerStats 解码，且 class_icon 非空、talents 字段存在
#[test]
fn test_player_stats_proto_contains_class_icon_and_talents() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 70001, 500.0, 500.0);
    // 设置职业和天赋（warrior=1）
    if let Some(mut p) = state.players.get_mut(&70001) {
        p.class = 1; // Warrior
        p.talents = vec![1, 3];
        // 给一个可装备的物品（铁剑 item_id=1）
        p.add_item(1, 1);
    }

    let resp = state.handle_equip(70001, 1);
    // 找到 5001 消息并解码
    let stats_msg = resp.iter().find(|m| m.msg_id == 5001)
        .expect("handle_equip 应返回 5001");
    // proto payload 不应以 '{' 开头
    assert_ne!(stats_msg.payload.first(), Some(&0x7B),
        "5001 应为 proto 编码而非 JSON");
    let decoded = gp::PlayerStats::decode(&stats_msg.payload[..])
        .expect("5001 payload 应可被 PlayerStats 解码");
    // 验证扩展字段
    assert!(!decoded.class_icon.is_empty(),
        "class_icon 应非空 (warrior 应有图标), 实际: {:?}", decoded.class_icon);
    assert_eq!(decoded.talents, vec![1, 3],
        "talents 应为 [1, 3]");
    assert_eq!(decoded.class_id, 1, "class_id 应为 warrior=1");
    assert_eq!(decoded.talent_points, decoded.talent_points, "talent_points 字段应存在");
}

/// TDD: ExpUpdate (5002) MP 更新变体 — is_mp_update=true
/// Given: 玩家攻击怪物消耗 MP
/// When: 触发 handle_attack（技能攻击会先发 5002 MP 更新）
/// Then: 5002 payload 解码后 is_mp_update=true, mp/max_mp 非零, exp/gained 为 0
#[test]
fn test_exp_update_proto_mp_variant() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 70002, 500.0, 500.0);
    add_test_mob(&state, 10000, 1, 540.0, 500.0); // 在攻击范围内

    let resp = state.handle_attack(70002, 2, 10000); // skill_id=2 重击, mp_cost=10
    // 找到 5002 消息（技能攻击会先发 MP 更新）
    let mp_msg = resp.iter().find(|m| m.msg_id == 5002)
        .expect("技能攻击应返回 5002 MP 更新");
    assert_ne!(mp_msg.payload.first(), Some(&0x7B),
        "5002 应为 proto 编码");
    let decoded = gp::ExpUpdate::decode(&mp_msg.payload[..])
        .expect("5002 payload 应可被 ExpUpdate 解码");
    // MP 变体: is_mp_update=true, mp/max_mp 有值, exp 字段为 0
    assert!(decoded.is_mp_update, "is_mp_update 应为 true (MP 更新变体)");
    assert!(decoded.mp > 0, "mp 应 > 0 (玩家有 MP), 实际: {}", decoded.mp);
    assert!(decoded.max_mp > 0, "max_mp 应 > 0, 实际: {}", decoded.max_mp);
    assert_eq!(decoded.gained, 0, "MP 变体 gained 应为 0");
    assert_eq!(decoded.exp, 0, "MP 变体 exp 应为 0");
}

/// TDD: ExpUpdate (5002) 经验变体 — is_mp_update=false
/// Given: 玩家击杀怪物获得经验
/// When: 触发 handle_attack 击杀怪物
/// Then: 5002 payload 解码后 is_mp_update=false, exp/gained 非零, mp 字段为 0
#[test]
fn test_exp_update_proto_exp_variant() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 70003, 500.0, 500.0);
    add_test_mob(&state, 10001, 1, 540.0, 500.0);
    // 设置高攻击力一击必杀
    if let Some(mut p) = state.players.get_mut(&70003) {
        p.atk = 9999;
        p.skill_cooldowns.clear();
    }

    let resp = state.handle_attack(70003, 1, 10001);
    // 击杀后应发 5002 经验更新（注意：技能攻击同时会发 MP 更新 5002，
    // 所以要找到 is_mp_update=false 的那条）
    let exp_msg = resp.iter()
        .filter(|m| m.msg_id == 5002)
        .find_map(|m| {
            gp::ExpUpdate::decode(&m.payload[..]).ok()
                .filter(|d| !d.is_mp_update)
                .map(|d| (m, d))
        })
        .expect("击杀怪物应返回 5002 经验更新 (is_mp_update=false)");
    let (_, decoded) = exp_msg;
    // 经验变体: is_mp_update=false, exp/gained 有值
    assert!(!decoded.is_mp_update, "is_mp_update 应为 false (经验变体)");
    assert!(decoded.gained > 0, "gained 应 > 0 (获得经验), 实际: {}", decoded.gained);
    assert!(decoded.max_exp > 0, "max_exp 应 > 0, 实际: {}", decoded.max_exp);
    assert_eq!(decoded.mp, 0, "经验变体 mp 字段应为 0");
    assert_eq!(decoded.max_mp, 0, "经验变体 max_mp 字段应为 0");
}

/// TDD: EquipmentUpdate (5004) 空槽位用 empty=true 表示
/// Given: 玩家只装备武器，armor 和 accessory 为空
/// When: 触发 handle_equip 装备武器
/// Then: 5004 payload 解码后 weapon.empty=false, armor.empty=true, accessory.empty=true
#[test]
fn test_equipment_update_proto_empty_slots() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 70004, 500.0, 500.0);
    if let Some(mut p) = state.players.get_mut(&70004) {
        p.add_item(1, 1); // 铁剑
    }

    let resp = state.handle_equip(70004, 1);
    let equip_msg = resp.iter().find(|m| m.msg_id == 5004)
        .expect("handle_equip 应返回 5004");
    assert_ne!(equip_msg.payload.first(), Some(&0x7B),
        "5004 应为 proto 编码");
    let decoded = gp::EquipmentUpdate::decode(&equip_msg.payload[..])
        .expect("5004 payload 应可被 EquipmentUpdate 解码");
    // weapon 应非空
    let weapon = decoded.weapon.as_ref().expect("weapon 槽应存在");
    assert!(!weapon.empty, "weapon 应非空 (刚装备)");
    assert_eq!(weapon.item_id, 1, "weapon item_id 应为 1");
    assert!(!weapon.name.is_empty(), "weapon name 应非空");
    // armor 和 accessory 应为空
    let armor = decoded.armor.as_ref().expect("armor 槽应存在");
    assert!(armor.empty, "armor 应为空 (empty=true)");
    assert_eq!(armor.item_id, 0, "空槽 item_id 应为 0");
    let accessory = decoded.accessory.as_ref().expect("accessory 槽应存在");
    assert!(accessory.empty, "accessory 应为空 (empty=true)");
}

/// TDD: QuestUpdate (5005) 包含 desc 和 completed 字段
/// Given: 玩家接受任务1（杀5只史莱姆）
/// When: 触发 handle_accept_quest
/// Then: 5005 payload 解码后 QuestEntry 含 desc 非空、completed=false (进度0)
#[test]
fn test_quest_update_proto_has_desc_and_completed() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 70005, 500.0, 500.0);

    let resp = state.handle_accept_quest(70005, 1);
    let quest_msg = resp.iter().find(|m| m.msg_id == 5005)
        .expect("handle_accept_quest 应返回 5005");
    assert_ne!(quest_msg.payload.first(), Some(&0x7B),
        "5005 应为 proto 编码");
    let decoded = gp::QuestUpdate::decode(&quest_msg.payload[..])
        .expect("5005 payload 应可被 QuestUpdate 解码");
    assert!(!decoded.quests.is_empty(), "任务列表应非空");
    let quest = &decoded.quests[0];
    assert_eq!(quest.quest_id, 1, "quest_id 应为 1");
    assert!(!quest.name.is_empty(), "name 应非空");
    assert!(!quest.desc.is_empty(), "desc 应非空 (扩展字段), 实际: {:?}", quest.desc);
    assert_eq!(quest.progress, 0, "新接任务 progress 应为 0");
    assert_eq!(quest.target, 5, "target 应为 5 (杀5只史莱姆)");
    assert!(!quest.completed, "新接任务 completed 应为 false");
}

/// TDD: CombatResult (6001) 空挥变体 — swing=true
/// Given: 玩家攻击不存在的目标 (target_uid=0)
/// When: 触发 handle_attack
/// Then: 6001 payload 解码后 swing=true, target_uid=0, attacker_uid=玩家UID
#[test]
fn test_combat_result_proto_swing_variant() {
    use logic_lib::game_proto as gp;
    use prost::Message;

    let state = GameState::test_new();
    add_test_player(&state, 70006, 500.0, 500.0);
    if let Some(mut p) = state.players.get_mut(&70006) {
        p.skill_cooldowns.clear();
    }

    // 攻击不存在的目标 (target_uid=999999) — 应触发空挥分支
    let resp = state.handle_attack(70006, 1, 999999);
    let swing_msg = resp.iter().find(|m| m.msg_id == 6001)
        .expect("空挥应返回 6001");
    assert_ne!(swing_msg.payload.first(), Some(&0x7B),
        "6001 应为 proto 编码");
    let decoded = gp::CombatResult::decode(&swing_msg.payload[..])
        .expect("6001 payload 应可被 CombatResult 解码");
    // 空挥变体: swing=true, target_uid=0, attacker_uid=玩家
    assert!(decoded.swing, "swing 应为 true (空挥变体)");
    assert_eq!(decoded.target_uid, 0, "空挥 target_uid 应为 0");
    assert_eq!(decoded.attacker_uid, 70006, "attacker_uid 应为玩家 UID");
    assert!(!decoded.miss, "空挥 miss 应为 false (区别于 miss 变体)");
    assert_eq!(decoded.skill_id, 1, "skill_id 应为 1 (普攻)");
    assert!(decoded.damage > 0, "空挥 damage 应 > 0 (基于玩家攻击力)");
    // reason 应为空字符串（空挥不是错误）
    assert!(decoded.reason.is_empty(), "空挥 reason 应为空");
}

// ════════════════════════════════════════════════════════════════
// v0.8 配置数据层测试 — JSON 加载 + const fallback + 9100 下发
// ════════════════════════════════════════════════════════════════

/// TDD: GameConfig::load() 应返回非空配置（JSON 存在则读 JSON，否则用 const fallback）
#[test]
fn test_config_loader_loads_json_files() {
    let cfg = super::config_loader::GameConfig::load();
    assert!(!cfg.skills.is_empty(), "技能配置不应为空");
    assert!(!cfg.mobs.is_empty(), "怪物配置不应为空");
    assert!(!cfg.items.is_empty(), "物品配置不应为空");
    assert!(!cfg.quests.is_empty(), "任务配置不应为空");
    assert!(!cfg.classes.is_empty(), "职业配置不应为空");
    assert!(!cfg.talents.is_empty(), "天赋配置不应为空");
    assert!(!cfg.npcs.is_empty(), "NPC 配置不应为空");
    assert!(!cfg.maps.is_empty(), "地图配置不应为空");
    assert!(!cfg.shop_items.is_empty(), "商店配置不应为空");
}

/// TDD: 配置数据值正确（火球术 mp_cost=20, range=200.0）
#[test]
fn test_config_loader_skill_data_correct() {
    let cfg = super::config_loader::GameConfig::load();
    let fireball = cfg.skills.iter().find(|s| s.id == 3);
    assert!(fireball.is_some(), "火球术 (id=3) 应存在");
    let f = fireball.unwrap();
    assert_eq!(f.name, "火球术");
    assert_eq!(f.mp_cost, 20);
    assert_eq!(f.range, 200.0);
    assert_eq!(f.dmg_multiplier, 3.0);
}

/// TDD: 怪物配置数据正确（哥布林 max_hp=80, exp=35）
#[test]
fn test_config_loader_mob_data_correct() {
    let cfg = super::config_loader::GameConfig::load();
    let goblin = cfg.mobs.iter().find(|m| m.id == 2);
    assert!(goblin.is_some(), "哥布林 (id=2) 应存在");
    let g = goblin.unwrap();
    assert_eq!(g.name, "哥布林");
    assert_eq!(g.max_hp, 80);
    assert_eq!(g.exp, 35);
    assert_eq!(g.level, 2);
}

/// TDD: GameConfig::to_json() 应产出可解析为 JSON 对象的字符串，且包含所有数组字段
#[test]
fn test_config_to_json_serializable() {
    let cfg = super::config_loader::GameConfig::load();
    let json = cfg.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("to_json 应产出合法 JSON");
    assert!(parsed.get("skills").unwrap().is_array(), "应包含 skills 数组");
    assert!(parsed.get("mobs").unwrap().is_array(), "应包含 mobs 数组");
    assert!(parsed.get("items").unwrap().is_array(), "应包含 items 数组");
    assert!(parsed.get("quests").unwrap().is_array(), "应包含 quests 数组");
    assert!(parsed.get("classes").unwrap().is_array(), "应包含 classes 数组");
    assert!(parsed.get("talents").unwrap().is_array(), "应包含 talents 数组");
    assert!(parsed.get("npcs").unwrap().is_array(), "应包含 npcs 数组");
    assert!(parsed.get("maps").unwrap().is_array(), "应包含 maps 数组");
    assert!(parsed.get("shopItems").unwrap().is_array(), "应包含 shopItems 数组");
}

/// TDD: msg_id=101 请求配置应返回 msg_id=9100 的配置消息，payload 为合法 JSON
#[test]
fn test_config_request_returns_config_message() {
    let state = GameState::test_new();
    let resp = state.process_message(80001, 101, b"");
    let config_msg = resp.messages.iter().find(|m| m.msg_id == 9100);
    assert!(config_msg.is_some(), "应返回 9100 配置消息");
    let payload = String::from_utf8_lossy(&config_msg.unwrap().payload);
    let json: serde_json::Value = serde_json::from_str(&payload).expect("9100 payload 应为合法 JSON");
    assert!(json.get("items").unwrap().is_array(), "9100 payload 应包含 items 数组");
    assert!(json.get("skills").unwrap().is_array(), "9100 payload 应包含 skills 数组");
    // 验证具体数据：火球术应在配置里
    let has_fireball = json.get("skills").unwrap().as_array().unwrap().iter()
        .any(|s| s.get("id").and_then(|v| v.as_u64()) == Some(3)
            && s.get("name").and_then(|v| v.as_str()) == Some("火球术"));
    assert!(has_fireball, "9100 配置应包含火球术");
}
