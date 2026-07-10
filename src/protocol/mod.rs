//! 私有游戏协议编解码模块
//!
//! 阶段2核心：16字节固定包头、长度解析、粘包处理、CRC校验、AES加密、合法性校验
//!
//! 包结构：
//! ```text
//! +--------+--------+--------+--------+--------+--------+--------+--------+
//! | Magic  | Magic  | Version| Reserve| MsgId  | MsgId  | BodyLen| BodyLen|
//! | (2B)   |        | (1B)   | (1B)   | (2B)   |        | (2B)   |        |
//! +--------+--------+--------+--------+--------+--------+--------+--------+
//! | CRC32  | CRC32  | CRC32  | CRC32  |   Encrypted Body (variable)       |
//! | (4B)   |        |        |        |                                     |
//! +--------+--------+--------+--------+--------+--------+--------+--------+
//! ```
//! 包头固定 16 字节，Body 最大 8KB

pub mod decoder;
pub mod encoder;
pub mod packet_struct;

pub use decoder::PacketDecoder;
pub use encoder::PacketEncoder;
pub use packet_struct::{Packet, PacketHeader, MsgId, PROTOCOL_VERSION, HEADER_SIZE, MAX_BODY_SIZE};
