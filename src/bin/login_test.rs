//! 登录服集成测试工具
//!
//! 直接通过 gRPC 调用登录服，绕过网关，测试核心业务逻辑。
//!
//! ```bash
//! # 先启动登录服: cargo run --bin login-server
//! # 然后运行测试: cargo run --bin login-test
//! ```

use rust_mmo_gate::grpc_router::proto::gate::{
    logic_service_client::LogicServiceClient, ForwardRequest,
};
use serde_json::Value;

const LOGIN_SERVER: &str = "http://127.0.0.1:50052";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔══════════════════════════════════════════════╗");
    println!("║   登录服集成测试                              ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();

    let mut passed = 0;
    let mut failed = 0;

    // ── 测试 1: 登录成功 ──
    match test_login().await {
        Ok((uid, token)) => {
            println!("  ✅ PASS  登录成功: uid={} token_len={}", uid, token.len());
            passed += 1;

            // ── 测试 2: 角色列表（空） ──
            match test_char_list(uid).await {
                Ok(chars) => {
                    println!("  ✅ PASS  角色列表: {} 个角色 (预期 0)", chars);
                    passed += 1;
                }
                Err(e) => {
                    println!("  ❌ FAIL  角色列表: {}", e);
                    failed += 1;
                }
            }

            // ── 测试 3: 创建角色 ──
            match test_create_char(uid, "测试战士", "warrior").await {
                Ok(char_id) => {
                    println!("  ✅ PASS  创建角色: charId={}", char_id);
                    passed += 1;

                    // ── 测试 4: 角色列表（有1个） ──
                    match test_char_list(uid).await {
                        Ok(chars) => {
                            if chars == 1 {
                                println!("  ✅ PASS  角色列表: {} 个角色 (预期 1)", chars);
                                passed += 1;
                            } else {
                                println!("  ❌ FAIL  角色列表: {} 个角色 (预期 1)", chars);
                                failed += 1;
                            }
                        }
                        Err(e) => {
                            println!("  ❌ FAIL  角色列表: {}", e);
                            failed += 1;
                        }
                    }

                    // ── 测试 5: 创建第2个角色 ──
                    match test_create_char(uid, "测试法师", "mage").await {
                        Ok(id2) => {
                            println!("  ✅ PASS  创建第2角色: charId={}", id2);
                            passed += 1;

                            if id2 == char_id + 1 {
                                println!("  ✅ PASS  charId 自增正确: {}→{}", char_id, id2);
                                passed += 1;
                            }
                        }
                        Err(e) => {
                            println!("  ❌ FAIL  创建角色: {}", e);
                            failed += 1;
                        }
                    }

                    // ── 测试 6: 选择角色进入世界 ──
                    match test_select_char(uid, char_id).await {
                        Ok(world_token) => {
                            println!("  ✅ PASS  进入世界: worldToken_len={}", world_token.len());
                            passed += 1;
                        }
                        Err(e) => {
                            println!("  ❌ FAIL  进入世界: {}", e);
                            failed += 1;
                        }
                    }

                    // ── 测试 7: 选不存在的角色 ──
                    match test_select_char(uid, 99999).await {
                        Ok(_) => {
                            println!("  ❌ FAIL  选不存在角色: 应该失败但返回了成功");
                            failed += 1;
                        }
                        Err(e) => {
                            println!("  ✅ PASS  选不存在角色: 正确报错 - {}", e);
                            passed += 1;
                        }
                    }
                }
                Err(e) => {
                    println!("  ❌ FAIL  创建角色: {}", e);
                    failed += 1;
                }
            }

            // ── 测试 8: Token 验证 ──
            match test_token_verify(uid, &token).await {
                Ok(valid_uid) => {
                    if valid_uid == uid {
                        println!("  ✅ PASS  Token验证: uid={} ✓", valid_uid);
                        passed += 1;
                    } else {
                        println!("  ❌ FAIL  Token验证: uid={} (预期 {})", valid_uid, uid);
                        failed += 1;
                    }
                }
                Err(e) => {
                    println!("  ❌ FAIL  Token验证: {}", e);
                    failed += 1;
                }
            }
        }
        Err(e) => {
            println!("  ❌ FAIL  登录: {}", e);
            failed += 1;
        }
    }

    // ── 测试 9: 密码错误 ──
    match test_login_fail("test", "wrong_password").await {
        Ok(_) => {
            println!("  ❌ FAIL  密码错误: 应该失败但返回了成功");
            failed += 1;
        }
        Err(e) => {
            println!("  ✅ PASS  密码错误: 正确报错 - {}", e);
            passed += 1;
        }
    }

    // ── 测试 10: 注册新用户 ──
    match test_register().await {
        Ok((new_uid, token)) => {
            println!("  ✅ PASS  注册: uid={} token_len={}", new_uid, token.len());
            passed += 1;

            // 用新账户登录
            match test_login_custom("newplayer_test", "password123").await {
                Ok((uid2, _)) => {
                    if uid2 == new_uid {
                        println!("  ✅ PASS  注册后登录: uid 一致 {}", uid2);
                        passed += 1;
                    } else {
                        println!("  ❌ FAIL  注册后登录: uid={} (预期 {})", uid2, new_uid);
                        failed += 1;
                    }
                }
                Err(e) => {
                    println!("  ❌ FAIL  注册后登录: {}", e);
                    failed += 1;
                }
            }
        }
        Err(e) => {
            println!("  ❌ FAIL  注册: {}", e);
            failed += 1;
        }
    }

    println!();
    println!("══════════════════════════════════════════════");
    println!("  结果: {} 通过, {} 失败, {} 总计", passed, failed, passed + failed);
    if failed == 0 {
        println!("  🎉 全部通过!");
    }
    println!("══════════════════════════════════════════════");

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// 发送 gRPC forward_message 请求并解析响应
async fn send_msg(
    uid: u64,
    msg_id: u32,
    payload: &str,
) -> Result<Vec<(u64, u32, Value)>, String> {
    let mut client = LogicServiceClient::connect(LOGIN_SERVER)
        .await
        .map_err(|e| format!("连接失败: {}", e))?;

    let request = tonic::Request::new(ForwardRequest {
        player_uid: uid,
        msg_id,
        payload: payload.as_bytes().to_vec(),
    });

    let response = client
        .forward_message(request)
        .await
        .map_err(|e| format!("gRPC调用失败: {}", e))?;

    let resp = response.into_inner();
    let mut results = Vec::new();

    for msg in resp.messages {
        let json: Value =
            serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);
        results.push((msg.target_uid, msg.msg_id, json));
    }

    Ok(results)
}

/// 从 DownstreamMessage 中提取 JSON 值
fn get_json(msgs: &[(u64, u32, Value)], msg_id: u32) -> Option<Value> {
    msgs.iter()
        .find(|(_, id, _)| *id == msg_id)
        .map(|(_, _, json)| json.clone())
}

/// 从 DownstreamMessage 中提取错误信息
fn get_error(msgs: &[(u64, u32, Value)], msg_id: u32) -> Option<String> {
    get_json(msgs, msg_id)
        .and_then(|j| j.get("error").and_then(|e| e.as_str()).map(|s| s.to_string()))
}

// ════════════════════════════════════════════════════════════════
// 测试用例
// ════════════════════════════════════════════════════════════════

/// 测试 1: 正常登录
async fn test_login() -> Result<(u64, String), String> {
    let msgs = send_msg(10001, 101, r#"{"username":"test","password":"123456"}"#).await?;

    if let Some(err) = get_error(&msgs, 5101) {
        return Err(err);
    }

    let json = get_json(&msgs, 5101).ok_or("未收到 5101 响应")?;
    let success = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    if !success {
        return Err("登录失败".to_string());
    }

    let uid = json.get("uid").and_then(|v| v.as_u64()).unwrap_or(0);
    let token = json.get("token").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if uid == 0 || token.is_empty() {
        return Err("uid 或 token 为空".to_string());
    }

    Ok((uid, token))
}

/// 测试: 密码错误
async fn test_login_fail(username: &str, password: &str) -> Result<(), String> {
    let payload = serde_json::json!({"username": username, "password": password}).to_string();
    let msgs = send_msg(10001, 101, &payload).await?;

    if let Some(err) = get_error(&msgs, 5101) {
        return Err(err); // 预期失败
    }

    let json = get_json(&msgs, 5101).ok_or("未收到响应")?;
    let success = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    if success {
        return Ok(()); // 不应该成功
    }

    Err("预期失败但未收到错误".to_string())
}

/// 测试: 自定义用户名登录
async fn test_login_custom(username: &str, password: &str) -> Result<(u64, String), String> {
    let payload = serde_json::json!({"username": username, "password": password}).to_string();
    let msgs = send_msg(10001, 101, &payload).await?;

    if let Some(err) = get_error(&msgs, 5101) {
        return Err(err);
    }

    let json = get_json(&msgs, 5101).ok_or("未收到响应")?;
    let uid = json.get("uid").and_then(|v| v.as_u64()).unwrap_or(0);
    let token = json.get("token").and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok((uid, token))
}

/// 测试 2: 角色列表
async fn test_char_list(uid: u64) -> Result<usize, String> {
    let msgs = send_msg(uid, 103, "{}").await?;

    if let Some(err) = get_error(&msgs, 5103) {
        return Err(err);
    }

    let json = get_json(&msgs, 5103).ok_or("未收到 5103 响应")?;
    let chars = json.get("chars").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    Ok(chars)
}

/// 测试 3: 创建角色
async fn test_create_char(uid: u64, name: &str, class: &str) -> Result<u64, String> {
    let payload = serde_json::json!({"name": name, "class": class}).to_string();
    let msgs = send_msg(uid, 104, &payload).await?;

    if let Some(err) = get_error(&msgs, 5104) {
        return Err(err);
    }

    let json = get_json(&msgs, 5104).ok_or("未收到 5104 响应")?;
    let char_id = json
        .get("char")
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if char_id == 0 {
        return Err("charId 为 0".to_string());
    }

    Ok(char_id)
}

/// 测试 6: 选择角色进入世界
async fn test_select_char(uid: u64, char_id: u64) -> Result<String, String> {
    let payload = serde_json::json!({"charId": char_id}).to_string();
    let msgs = send_msg(uid, 105, &payload).await?;

    if let Some(err) = get_error(&msgs, 5105) {
        return Err(err);
    }

    let json = get_json(&msgs, 5105).ok_or("未收到 5105 响应")?;
    let token = json.get("worldToken").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if token.is_empty() {
        return Err("worldToken 为空".to_string());
    }
    Ok(token)
}

/// 测试 8: Token 验证
async fn test_token_verify(uid: u64, token: &str) -> Result<u64, String> {
    let payload = serde_json::json!({"token": token}).to_string();
    let msgs = send_msg(uid, 106, &payload).await?;

    if let Some(err) = get_error(&msgs, 5106) {
        return Err(err);
    }

    let json = get_json(&msgs, 5106).ok_or("未收到 5106 响应")?;
    let valid_uid = json.get("uid").and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(valid_uid)
}

/// 测试 10: 注册新用户
async fn test_register() -> Result<(u64, String), String> {
    let payload = r#"{"username":"newplayer_test","password":"password123"}"#;
    let msgs = send_msg(10001, 102, payload).await?;

    if let Some(err) = get_error(&msgs, 5102) {
        return Err(err);
    }

    let json = get_json(&msgs, 5102).ok_or("未收到 5102 响应")?;
    let uid = json.get("uid").and_then(|v| v.as_u64()).unwrap_or(0);
    let token = json.get("token").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if uid == 0 {
        return Err("注册uid为0".to_string());
    }

    Ok((uid, token))
}
