//! 协议的时间常量。
//!
//! 它们**属于协议**（两侧必须用同一组值），所以放在 `proto` 而不是某一侧的运行时代码里 ——
//! agent 侧同名的常量在 `agent/src/main/kotlin/.../proto/Timing.kt`。
//!
//! 依据 `docs/spec/agent/runtime-observability.md` §2：
//! 心跳周期固定 **3s**；判活判据是「`T_dead`（**10s**）内收到对端**任何**消息」，
//! **不是**「收到心跳」—— 对端忙碌时不必额外发心跳。

use std::time::Duration;

/// 心跳周期：**双向**每 3s 一条。
pub const HEARTBEAT_PERIOD: Duration = Duration::from_secs(3);

/// 判活超时 `T_dead`：这么久没收到对端任何消息即判掉线。
///
/// 它必须 **> 心跳周期**，否则正常的心跳间隔本身就会被判死。
pub const T_DEAD: Duration = Duration::from_secs(10);
