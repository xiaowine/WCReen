//! WCReen 线协议（PC 侧实现）。
//!
//! 规范：`docs/spec/agent/transport-protocol.md`（帧框架 / 控制消息格式）、
//! `docs/spec/agent/transport-messages.md`（每条消息的字段）。
//! agent 侧的实现见 `agent/src/main/kotlin/com/xiaowine/wcreen/agent/proto/*.kt`，
//! **本 crate 与它逐字节对齐**（CRC 的覆盖范围、字节序、字段名都必须一致）。
//!
//! # 三层结构
//!
//! | 模块 | 负责 |
//! |---|---|
//! | [`wire`] | 帧框架 `[1B 类型][4B 小端长度][负载]` + 帧（`0x01`）的负载结构 |
//! | [`control`] | 控制消息负载 `[4B 小端 CRC32][UTF-8 JSON]`（CRC 覆盖 `[类型][长度][JSON]`） |
//! | [`dto`] | 各控制消息的 JSON 结构（字段名与协议逐字一致） |
//!
//! # 为什么 CRC 覆盖头部
//!
//! 控制消息的 CRC 是**失步检测器 + 实现自检**，不是防线路噪声（传输层已有校验）。
//! 读循环一旦读错长度，后面全部乱套且**无法自我恢复** —— 所以 CRC 必须连
//! `[类型][长度]` 一起保护：头部损坏最危险。
//!
//! # 本 crate 的边界
//!
//! **纯逻辑**：只依赖 `serde` / `serde_json` / `crc32fast`，不碰 FFmpeg、egui、wgpu、tokio。
//! 所有失败都是 [`ProtocolError`]，**不 panic** —— 协议错误必须能被上层翻译成
//! 「断连 / 重连」而不是把进程带走。

#![deny(missing_docs)]

pub mod control;
pub mod dto;
pub mod error;
pub mod timing;
pub mod wire;

pub use control::{crc32, decode_control, encode_control, CRC_SIZE};
pub use dto::{
    AppliedScreen, ApplyScreenArgs, ApplyScreenResult, BitrateMode, Caps, CapsDefaults,
    CapsRequest, Codec, CodecCaps, ColorRange, ColorStandard, ColorTransfer, Command, CommandAck,
    CommandBody, ErrorObj, Event, Heartbeat, Hello, HelloAck, InputAction, InputArgs, InputKey,
    InputPointer, InputScroll, IntRange, ScreenParams, PROTOCOL_VERSION,
};
pub use error::ProtocolError;
pub use wire::{
    encode_video_payload, parse_frame_header, parse_video_payload, read_frame, write_frame, Frame,
    FrameHeader, VideoFrameHeader, FLAG_KEYFRAME, HEADER_SIZE, MAX_PAYLOAD, VIDEO_HEADER_SIZE,
};
