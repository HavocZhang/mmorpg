//! 应用配置结构定义与加载逻辑
//!
//! 支持三套环境：dev / staging / prod
//! 通过环境变量 + .env 文件加载，无需修改代码切换环境

use anyhow::Result;
use serde::Deserialize;

/// 顶层配置根结构
#[derive(Clone, Debug, Deserialize)]
pub struct AppConfig {
    pub app: AppSection,
    pub gate: GateSection,
    pub redis: RedisSection,
    pub grpc: GrpcSection,
    pub rate_limit: RateLimitSection,
    pub session: SessionSection,
    pub protocol: ProtocolSection,
    pub crypto: CryptoSection,
    pub io: IoSection,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AppSection {
    pub env: String,
    pub log_level: String,
    pub log_format: String,
    pub tokio_worker_threads: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GateSection {
    pub tcp_bind: String,
    pub tcp_port: u16,
    pub http_bind: String,
    pub http_port: u16,
    pub node_id: u64,
    pub node_name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RedisSection {
    pub url: String,
    pub cluster: bool,
    pub pool_size: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GrpcSection {
    pub logic_endpoints: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RateLimitSection {
    pub player_per_sec: u32,
    pub player_battle_per_sec: u32,
    pub global_per_sec: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionSection {
    pub heartbeat_timeout_secs: u64,
    pub heartbeat_check_interval_secs: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProtocolSection {
    pub max_packet_size: usize,
    pub packet_magic: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CryptoSection {
    pub aes_key: String,
    pub aes_nonce: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct IoSection {
    pub packet_merge_window_ms: u64,
}

impl AppConfig {
    /// 从环境变量加载配置
    ///
    /// 加载顺序：
    /// 1. 先加载对应环境的 .env 文件
    /// 2. 再从环境变量读取（可覆盖 .env 值）
    pub fn load() -> Result<Self> {
        // 确定环境，默认 dev
        let env = std::env::var("APP_ENV").unwrap_or_else(|_| "dev".to_string());

        // 加载对应 .env 文件
        let env_file = format!(".env.{}", env);
        let _ = dotenv::from_filename(&env_file);

        // 从环境变量构建配置
        let config = Self::from_env()?;
        Ok(config)
    }

    /// 从环境变量逐一读取构建配置
    fn from_env() -> Result<Self> {
        let get = |key: &str, default: &str| {
            std::env::var(key).unwrap_or_else(|_| default.to_string())
        };

        let get_u32 = |key: &str, default: u32| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };

        let get_u64 = |key: &str, default: u64| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };

        let get_usize = |key: &str, default: usize| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };

        let get_u16 = |key: &str, default: u16| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };

        Ok(AppConfig {
            app: AppSection {
                env: get("APP_ENV", "dev"),
                log_level: get("LOG_LEVEL", "info"),
                log_format: get("LOG_FORMAT", "pretty"),
                tokio_worker_threads: get_usize("TOKIO_WORKER_THREADS", 4),
            },
            gate: GateSection {
                tcp_bind: get("GATE_TCP_BIND", "0.0.0.0"),
                tcp_port: get_u16("GATE_TCP_PORT", 7888),
                http_bind: get("GATE_HTTP_BIND", "0.0.0.0"),
                http_port: get_u16("GATE_HTTP_PORT", 9090),
                node_id: get_u64("GATE_NODE_ID", 1),
                node_name: get("GATE_NODE_NAME", "gate-01"),
            },
            redis: RedisSection {
                url: get("REDIS_URL", "redis://127.0.0.1:6379"),
                cluster: get("REDIS_CLUSTER", "false") == "true",
                pool_size: get_usize("REDIS_POOL_SIZE", 8),
            },
            grpc: GrpcSection {
                logic_endpoints: get("GRPC_LOGIC_ENDPOINTS", "grpc://127.0.0.1:50051"),
            },
            rate_limit: RateLimitSection {
                player_per_sec: get_u32("RATE_LIMIT_PLAYER_PER_SEC", 30),
                player_battle_per_sec: get_u32("RATE_LIMIT_PLAYER_BATTLE_PER_SEC", 80),
                global_per_sec: get_u32("RATE_LIMIT_GLOBAL_PER_SEC", 80000),
            },
            session: SessionSection {
                heartbeat_timeout_secs: get_u64("SESSION_HEARTBEAT_TIMEOUT_SECS", 45),
                heartbeat_check_interval_secs: get_u64("SESSION_HEARTBEAT_CHECK_INTERVAL_SECS", 10),
            },
            protocol: ProtocolSection {
                max_packet_size: get_usize("PROTOCOL_MAX_PACKET_SIZE", 8192),
                packet_magic: get_u32("PROTOCOL_PACKET_MAGIC", 0x4D4D4F47),
            },
            crypto: CryptoSection {
                aes_key: get("AES_KEY", "00112233445566778899aabbccddeeff"),
                aes_nonce: get("AES_NONCE", "0011223344556677"),
            },
            io: IoSection {
                packet_merge_window_ms: get_u64("PACKET_MERGE_WINDOW_MS", 16),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_load_dev() {
        std::env::set_var("APP_ENV", "dev");
        let config = AppConfig::load();
        assert!(config.is_ok(), "dev 配置加载应该成功");
        let config = config.unwrap();
        assert_eq!(config.app.env, "dev");
    }

    #[test]
    fn test_config_defaults() {
        // 验证默认值合理性
        let config = AppConfig::from_env();
        assert!(config.is_ok(), "从环境变量加载配置应该成功");
    }
}
