//! 协议层的失败类型。
//!
//! 全部失败都是**可恢复的协议级错误** —— 上层（连接层）把它们翻译成
//! 「断开这条连接 / 重连」，而不是让进程崩掉。协议层**不做任何重试**：
//! 重试策略属于连接层（那里才知道连接状态）。

use std::fmt;

/// 协议编解码的失败。
///
/// 每个变体都对应一个**可诊断**的原因：要么是线格式不合法，要么是对端行为违反协议。
#[derive(Debug)]
pub enum ProtocolError {
    /// 底层 IO 失败（对端关闭、超时、网络错误）。
    Io(std::io::Error),
    /// 对端在帧头或负载中途关闭了连接。
    PeerClosed {
        /// 已经读到的字节数。
        got: usize,
        /// 期望读满的字节数。
        expected: usize,
    },
    /// 对端关闭了连接（收帧循环在帧边界上读到 EOF）。
    ///
    /// 与 [`Self::PeerClosed`] 分开：那条说的是「读到一半断了」，这条说的是
    /// 「对端干净地关了连接」—— 而后者最常见的成因是**对端根本不是我们要找的那个服务**
    /// （例如 `adb forward` 还在、设备上却没有 agent 在监听），所以诊断信息不一样。
    Closed {
        /// 关闭时缓冲里还没凑成一帧的字节数（`0` = 恰好在帧边界上）。
        pending: usize,
    },
    /// 头部声明的负载长度超过上限。
    ///
    /// **这条在分配之前就返回** —— 对端声明一个巨大长度是典型的炸内存手法
    /// （`transport-protocol.md` §1），所以绝不能先分配再检查。
    PayloadTooLarge {
        /// 头部声明的长度。
        length: u64,
        /// 允许的上限。
        limit: usize,
    },
    /// 负载短于该类型的最小长度（例如控制消息负载放不下 4 字节 CRC）。
    PayloadTooShort {
        /// 至少要多少字节。
        need: usize,
        /// 实际多少字节。
        got: usize,
    },
    /// 负载长度不是 4 的倍数这类结构性问题（目前只有帧负载使用）。
    Malformed {
        /// 可诊断的说明。
        detail: String,
    },
    /// CRC32 不匹配 —— 读循环已经失步，或对端实现与本端不一致。
    ///
    /// 处置只能是**断开重连**：长度前缀 + CRC 不提供重同步锚点。
    CrcMismatch {
        /// 负载里携带的 CRC。
        expected: u32,
        /// 本端算出的 CRC。
        actual: u32,
    },
    /// JSON 序列化 / 反序列化失败。
    Json {
        /// 可诊断的说明（含 serde 的原文）。
        detail: String,
    },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO 失败：{e}"),
            Self::PeerClosed { got, expected } => {
                write!(f, "对端提前关闭连接（期望 {expected} 字节，只读到 {got}）")
            }
            Self::Closed { pending: 0 } => write!(f, "对端关闭了连接"),
            Self::Closed { pending } => write!(
                f,
                "对端关闭了连接（关闭时还有 {pending} 字节没凑成一帧）"
            ),
            Self::PayloadTooLarge { length, limit } => {
                write!(f, "负载长度 {length} 超过上限 {limit} ⇒ 拒绝分配（协议级错误）")
            }
            Self::PayloadTooShort { need, got } => {
                write!(f, "负载只有 {got} 字节，不足所需的 {need} 字节")
            }
            Self::Malformed { detail } => write!(f, "负载结构非法：{detail}"),
            Self::CrcMismatch { expected, actual } => write!(
                f,
                "CRC32 不匹配（收到 {expected:#010x}，算出 {actual:#010x}）\
                 —— 读循环已失步或对端实现不一致，只能断连重连"
            ),
            Self::Json { detail } => write!(f, "JSON 编解码失败：{detail}"),
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ProtocolError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for ProtocolError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json { detail: e.to_string() }
    }
}
