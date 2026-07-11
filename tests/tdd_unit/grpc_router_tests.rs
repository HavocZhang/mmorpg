//! TDD 单元测试 — gRPC路由模块
//!
//! 测试连接池轮询、健康检查、一致性哈希路由、端点标记

use rust_mmo_gate::grpc_router::conn_pool::GrpcConnPool;

#[test]
fn test_grpc_pool_new_with_endpoints() {
    let pool = GrpcConnPool::new(vec!["grpc://127.0.0.1:50051".into()]);
    assert_eq!(pool.total_count(), 1);
    assert_eq!(pool.healthy_count(), 1);
}

#[test]
fn test_grpc_pool_round_robin() {
    let pool = GrpcConnPool::new(vec![
        "grpc://1.1.1.1:50051".into(),
        "grpc://2.2.2.2:50051".into(),
        "grpc://3.3.3.3:50051".into(),
    ]);
    let ep1 = pool.next_endpoint().unwrap();
    let ep2 = pool.next_endpoint().unwrap();
    let ep3 = pool.next_endpoint().unwrap();
    let ep4 = pool.next_endpoint().unwrap();
    assert_eq!(ep1, ep4, "第三轮应回到第一个端点");
    assert_ne!(ep1, ep2);
    assert_ne!(ep2, ep3);
}

#[test]
fn test_grpc_pool_route_by_key_consistent() {
    let pool = GrpcConnPool::new(vec![
        "grpc://1.1.1.1:50051".into(),
        "grpc://2.2.2.2:50051".into(),
    ]);
    let ep1 = pool.route_by_key("player_123").unwrap();
    let ep2 = pool.route_by_key("player_123").unwrap();
    assert_eq!(ep1, ep2, "相同key应路由到相同端点");
}

#[test]
fn test_grpc_pool_mark_unhealthy_excludes_from_routing() {
    let pool = GrpcConnPool::new(vec![
        "grpc://1.1.1.1:50051".into(),
        "grpc://2.2.2.2:50051".into(),
    ]);
    assert_eq!(pool.healthy_count(), 2);
    pool.mark_unhealthy("grpc://1.1.1.1:50051");
    assert_eq!(pool.healthy_count(), 1);
    for _ in 0..10 {
        let ep = pool.next_endpoint().unwrap();
        assert_ne!(ep, "grpc://1.1.1.1:50051");
    }
}

#[test]
fn test_grpc_pool_mark_healthy_restores_endpoint() {
    let pool = GrpcConnPool::new(vec!["grpc://127.0.0.1:50051".into()]);
    pool.mark_unhealthy("grpc://127.0.0.1:50051");
    assert_eq!(pool.healthy_count(), 0);
    pool.mark_healthy("grpc://127.0.0.1:50051");
    assert_eq!(pool.healthy_count(), 1);
}

#[test]
fn test_grpc_pool_all_unhealthy_returns_none() {
    let pool = GrpcConnPool::new(vec!["grpc://127.0.0.1:50051".into()]);
    pool.mark_unhealthy("grpc://127.0.0.1:50051");
    assert!(pool.next_endpoint().is_none());
    assert!(pool.route_by_key("test").is_none());
}

#[test]
fn test_grpc_pool_has_healthy_endpoint() {
    let pool = GrpcConnPool::new(vec!["grpc://127.0.0.1:50051".into()]);
    assert!(pool.has_healthy_endpoint());
    pool.mark_unhealthy("grpc://127.0.0.1:50051");
    assert!(!pool.has_healthy_endpoint());
}

#[test]
fn test_grpc_pool_empty_pool() {
    let pool = GrpcConnPool::new(vec![]);
    assert!(pool.next_endpoint().is_none());
    assert_eq!(pool.total_count(), 0);
    assert_eq!(pool.healthy_count(), 0);
}

#[test]
fn test_grpc_pool_mark_unhealthy_nonexistent_endpoint() {
    let pool = GrpcConnPool::new(vec!["grpc://127.0.0.1:50051".into()]);
    // 不应 panic
    pool.mark_unhealthy("grpc://nonexistent:9999");
    assert_eq!(pool.healthy_count(), 1);
}
