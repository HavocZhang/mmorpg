//! gRPC proto 生成代码模块
//!
//! 由 build.rs 从 proto/gate.proto 编译生成
//! 包含 LogicService 服务定义和消息结构

pub mod gate {
    // Include the generated code from OUT_DIR
    include!(concat!(env!("OUT_DIR"), "/gate.rs"));
}

pub use gate::{
    DownstreamMessage, ForwardBatchRequest, ForwardRequest, ForwardResponse,
    PlayerOfflineRequest, PlayerOfflineResponse, PlayerOnlineRequest, PlayerOnlineResponse,
};
