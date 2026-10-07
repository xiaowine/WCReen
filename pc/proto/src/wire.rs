//! 帧框架与视频帧负载。
//!
//! ```text
//! [1B 类型][4B 负载长度 N（小端）][N 字节负载]
//! ```
//!
//! - 长度是**负载长度**，不含 5 字节头；
//! - 上限 [`MAX_PAYLOAD`]（16 MiB）—— 超限**在分配之前**就报错；
//! - 字节序**小端**（两侧都是小端机器，但按「不依赖默认值」显式写死）；
//! - 头定长 ⇒ 读循环不分叉；
//! - 写的时候**头单独写、负载再写** ⇒ 帧数据零拷贝直写（合并成一次写反而要拷整帧）。

use std::io::{Read, Write};

use crate::error::ProtocolError;

/// 帧头字节数：1 字节类型 + 4 字节负载长度。
pub const HEADER_SIZE: usize = 5;

/// 负载长度上限 16 MiB —— 超限即断连。
///
/// 协议不做分片（TCP / AF_UNIX 都是字节流，「长度前缀 + 读到够」天然解决任意大小），
/// 所以这是唯一的长度约束。它同时是**防炸内存**的手段。
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

/// 帧类型取值（`transport-protocol.md` §4 的类型表，共 9 个值）。
///
/// 按功能分段：`0x0x` 帧 / `0x1x` 握手与能力 / `0x2x` 指令与上报 / `0x3x` 心跳。
/// 「指令」只占一个类型值，具体是哪条指令放在 JSON 的 `cmd` 字段里。
pub mod kind {
    /// agent→PC 视频帧，负载是二进制（[`super::VideoFrameHeader`] + Annex-B）。
    pub const FRAME: u8 = 0x01;
    /// PC→agent 握手：协议版本。
    pub const HELLO: u8 = 0x10;
    /// agent→PC 握手应答：协议版本 + 自身状态。
    pub const HELLO_ACK: u8 = 0x11;
    /// PC→agent 请求能力表。
    pub const CAPS_REQUEST: u8 = 0x12;
    /// agent→PC 能力表。
    pub const CAPS: u8 = 0x13;
    /// PC→agent 指令（`cmd` 区分具体哪条）。
    pub const COMMAND: u8 = 0x20;
    /// agent→PC 指令回报。
    pub const COMMAND_ACK: u8 = 0x21;
    /// agent→PC 主动上报。
    pub const EVENT: u8 = 0x22;
    /// 双向心跳，负载是空 JSON 对象。
    pub const HEARTBEAT: u8 = 0x30;
}

/// 帧类型的中文名 —— **只用于日志**（未知类型给出十六进制原文，不猜测）。
pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        kind::FRAME => "视频帧",
        kind::HELLO => "握手",
        kind::HELLO_ACK => "握手应答",
        kind::CAPS_REQUEST => "请求能力表",
        kind::CAPS => "能力表",
        kind::COMMAND => "指令",
        kind::COMMAND_ACK => "指令回报",
        kind::EVENT => "主动上报",
        kind::HEARTBEAT => "心跳",
        _ => "未知类型",
    }
}

/// 帧头：类型 + 负载长度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// 帧类型（见 [`kind`]）。
    pub kind: u8,
    /// **负载**长度（不含 5 字节头）。
    pub length: u32,
}

impl FrameHeader {
    /// 编成 5 字节头（小端）。
    ///
    /// @return 长度超限时返回 [`ProtocolError::PayloadTooLarge`] —— 绝不静默截断。
    pub fn to_bytes(self) -> Result<[u8; HEADER_SIZE], ProtocolError> {
        check_length(self.length as u64)?;
        let mut out = [0u8; HEADER_SIZE];
        out[0] = self.kind;
        out[1..5].copy_from_slice(&self.length.to_le_bytes());
        Ok(out)
    }
}

/// 一帧：类型 + 负载。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 帧类型（见 [`kind`]）。
    pub kind: u8,
    /// 负载字节。
    pub payload: Vec<u8>,
}

/// 解析 5 字节帧头（小端）。
///
/// @param buf 恰好 5 字节的头部
/// @return 成功给出头部；长度不足或超限都报错
pub fn parse_frame_header(buf: &[u8]) -> Result<FrameHeader, ProtocolError> {
    if buf.len() < HEADER_SIZE {
        return Err(ProtocolError::PayloadTooShort { need: HEADER_SIZE, got: buf.len() });
    }
    let length = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
    check_length(length as u64)?;
    Ok(FrameHeader { kind: buf[0], length })
}

/// 读一帧（头 + 负载）。
///
/// 先读定长头 → 校验长度上限 → 再读负载 ⇒ **读循环不分叉**，且**超限时一个字节都不分配**。
///
/// ⚠️ 本函数会**阻塞到读满为止**：调用方负责先确认「有数据可读」（TCP 上用读超时，
/// 或者用 `poll`）—— 判活超时属于连接层，不属于协议层。
///
/// @param reader 连接读侧
/// @return 成功给出帧；对端关闭、IO 失败、长度越界都给出对应错误
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Frame, ProtocolError> {
    let mut header = [0u8; HEADER_SIZE];
    read_exact(reader, &mut header)?;
    let header = parse_frame_header(&header)?;

    let length = header.length as usize;
    let mut payload = vec![0u8; length];
    if length > 0 {
        read_exact(reader, &mut payload)?;
    }
    Ok(Frame { kind: header.kind, payload })
}

/// 写一帧（头单独写、负载再写）。
///
/// 分成两次写是为了**帧数据零拷贝直写**：合并成一次 `write_all` 就得先把整帧
/// 拷进一个临时缓冲，而视频帧是 16 MiB 级别的数据。
///
/// @param writer 连接写侧
/// @param kind 帧类型
/// @param payload 负载
pub fn write_frame<W: Write>(
    writer: &mut W,
    kind: u8,
    payload: &[u8],
) -> Result<(), ProtocolError> {
    let header = FrameHeader { kind, length: payload.len() as u32 }.to_bytes()?;
    writer.write_all(&header)?;
    if !payload.is_empty() {
        writer.write_all(payload)?;
    }
    writer.flush()?;
    Ok(())
}

// ---------------------------------------------------------------- 视频帧负载

/// 视频帧负载的固定头长度：1 字节 flags + 8 字节 pts_us。
pub const VIDEO_HEADER_SIZE: usize = 9;

/// 视频帧 `flags` 的 bit0 = 关键帧。
///
/// 关键帧**显式给**，不让接收端去扫 NAL —— 「不依赖默认值」。
pub const FLAG_KEYFRAME: u8 = 0x01;

/// 视频帧负载的固定头。
///
/// ```text
/// [1B flags][8B pts_us（小端）][Annex-B 数据]
/// ```
///
/// `pts_us` 是编码器给的 `presentationTimeUs`；裸 Annex-B 流**没有**容器时间戳，
/// 所以它是渲染节拍 / 抖动缓冲的**唯一**时间参考（帧率**不得**从裸流推导）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFrameHeader {
    /// 是否关键帧（`flags` bit0）。
    pub keyframe: bool,
    /// 呈现时刻（微秒）。
    pub pts_us: i64,
}

impl VideoFrameHeader {
    /// 编成 `flags` 字节。
    ///
    /// @return 只有 bit0 有定义，其余位保留为 0
    pub fn flags(self) -> u8 {
        if self.keyframe { FLAG_KEYFRAME } else { 0 }
    }
}

/// 编一个视频帧负载（`flags + pts_us + Annex-B`）。
///
/// 这是**发送侧**（agent）的写法；PC 侧只解析，不构造。放在这里是为了让协议
/// 的两端共用同一份格式定义，并被同一组单测覆盖。
///
/// @param keyframe 是否关键帧
/// @param pts_us 呈现时刻（微秒）
/// @param annex_b Annex-B 码流数据
/// @return 完整负载
pub fn encode_video_payload(keyframe: bool, pts_us: i64, annex_b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(VIDEO_HEADER_SIZE + annex_b.len());
    out.push(VideoFrameHeader { keyframe, pts_us }.flags());
    out.extend_from_slice(&pts_us.to_le_bytes());
    out.extend_from_slice(annex_b);
    out
}

/// 解析视频帧负载，**借用**出 Annex-B 数据（不拷贝）。
///
/// @param payload 类型为 [`kind::FRAME`] 的帧负载
/// @return 成功给出（固定头，Annex-B 切片）
pub fn parse_video_payload(payload: &[u8]) -> Result<(VideoFrameHeader, &[u8]), ProtocolError> {
    if payload.len() < VIDEO_HEADER_SIZE {
        return Err(ProtocolError::PayloadTooShort { need: VIDEO_HEADER_SIZE, got: payload.len() });
    }
    let flags = payload[0];
    let pts_us = i64::from_le_bytes([
        payload[1], payload[2], payload[3], payload[4], payload[5], payload[6], payload[7],
        payload[8],
    ]);
    let header = VideoFrameHeader { keyframe: flags & FLAG_KEYFRAME != 0, pts_us };
    Ok((header, &payload[VIDEO_HEADER_SIZE..]))
}

// ---------------------------------------------------------------- 内部

/// 校验负载长度上限。
///
/// 用 `u64` 接参数：头部是 32 位，Java 侧把它读成**有符号** `Int`，负值（≥ 2 GiB）
/// 换算成 `u64` 后必然远超上限 ⇒ 两侧对「超限」的判定一致。
fn check_length(length: u64) -> Result<(), ProtocolError> {
    if length > MAX_PAYLOAD as u64 {
        return Err(ProtocolError::PayloadTooLarge { length, limit: MAX_PAYLOAD });
    }
    Ok(())
}

/// 读满 `buf`。
///
/// 不用 `Read::read_exact` 是为了在**对端提前关闭**时能报出「已经读到多少」——
/// 那是判断失步位置的关键诊断信息。
fn read_exact<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<(), ProtocolError> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => {
                return Err(ProtocolError::PeerClosed { got: filled, expected: buf.len() });
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(ProtocolError::Io(e)),
        }
    }
    Ok(())
}
