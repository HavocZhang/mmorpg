//! 读写双循环模块
//!
//! 阶段5核心：收包循环、发包循环、16ms合并、优先级队列、拥堵降级
//!
//! - ReadLoop：独立异步任务，读取TCP数据 -> 解码 -> 路由上行
//! - WriteLoop：独立异步任务，从通道读取 -> 小包合并 -> 优先级排序 -> 发送

pub mod msg_priority;
pub mod packet_merge;
pub mod read_loop;
pub mod write_loop;
