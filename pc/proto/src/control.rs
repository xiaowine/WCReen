//! 控制消息负载的编解码：`[4B 小端 CRC32][UTF-8 JSON]`。
//!
//! # CRC 覆盖 `[类型][长度][JSON]` —— 连头部一起保护
//!
//! 头部的 `[类型][长度]` 损坏最危险（读循环会立刻失步），所以 CRC 把它们也纳入。
//! 长度字段喂进 CRC 的是**整个负载长度**（含 CRC 那 4 字节）—— 与 agent 侧
//! `Codec.encode` 逐字节一致，改一处就必须改两处。
//!
//! # 为什么不是「防线路噪声」
//!
//! 传输层（adb / TCP）已经防了字节被改。CRC 在这里是**失步检测器 + 实现自检**：
//! 读循环一旦读错长度，后面全部乱套且**无法自我恢复**，而失步是静默的。

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::ProtocolError;
use crate::wire::MAX_PAYLOAD;

/// 控制消息负载里 CRC 字段占的字节数。
pub const CRC_SIZE: usize = 4;

/// 计算控制消息的 CRC32。
///
/// 覆盖范围 = `[1B 类型][4B 小端负载长度][JSON]`，即**连头部一起保护**。
/// 算法是标准 CRC-32（IEEE 802.3，反射、初值 / 终值全 1）—— 与
/// `java.util.zip.CRC32` 一致（agent 侧用的就是它）。
///
/// @param kind 帧类型（1 字节）
/// @param payload_len 整个负载长度（= 4 + JSON 长度）
/// @param body JSON 字节
/// @return CRC32 值
pub fn crc32(kind: u8, payload_len: usize, body: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&[kind]);
    hasher.update(&(payload_len as u32).to_le_bytes());
    hasher.update(body);
    hasher.finalize()
}

/// 把一条控制消息编成负载。
///
/// @param kind 帧类型（参与 CRC）
/// @param msg 任意可序列化的 DTO
/// @return `[4B 小端 CRC32][UTF-8 JSON]`；JSON 超过长度上限时返回
///   [`ProtocolError::PayloadTooLarge`]（绝不截断）
pub fn encode_control<T: Serialize>(kind: u8, msg: &T) -> Result<Vec<u8>, ProtocolError> {
    let json = serde_json::to_vec(msg)?;
    let payload_len = CRC_SIZE + json.len();
    if payload_len > MAX_PAYLOAD {
        return Err(ProtocolError::PayloadTooLarge {
            length: payload_len as u64,
            limit: MAX_PAYLOAD,
        });
    }
    let crc = crc32(kind, payload_len, &json);
    let mut out = Vec::with_capacity(payload_len);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&json);
    Ok(out)
}

/// 校验 CRC 并反序列化。
///
/// **CRC 先于 JSON**：校验不过就绝不解析 —— 失步后的字节流交给 serde 只会
/// 产生一堆误导性的解析错误。
///
/// @param kind 帧类型（参与 CRC）
/// @param payload 收到的完整负载（含首 4 字节 CRC）
/// @return 成功给出 DTO；长度不足 / CRC 不匹配 / JSON 非法都给出对应错误
pub fn decode_control<T: DeserializeOwned>(kind: u8, payload: &[u8]) -> Result<T, ProtocolError> {
    if payload.len() < CRC_SIZE {
        return Err(ProtocolError::PayloadTooShort { need: CRC_SIZE, got: payload.len() });
    }
    let expected = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let body = &payload[CRC_SIZE..];
    let actual = crc32(kind, payload.len(), body);
    if expected != actual {
        return Err(ProtocolError::CrcMismatch { expected, actual });
    }
    serde_json::from_slice(body).map_err(ProtocolError::from)
}
