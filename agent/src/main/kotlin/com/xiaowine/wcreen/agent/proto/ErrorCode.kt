package com.xiaowine.wcreen.agent.proto

/**
 * 协议里 `error.code` 的取值表（`transport-messages.md` §4.1）。
 *
 * `code` 是**稳定的机器可读标识** —— PC 靠它分支，所以不能随手改字符串。
 * 本对象只登记**会出现在线上**的码；纯进程内的失败码在 `runtime.FailCode`。
 */
internal object ErrorCode {

    /** 协议版本不匹配。回报双方版本号后断开该连接。 */
    const val VERSION_MISMATCH: String = "versionMismatch"

    /** 收到的 `cmd` 不在本实现支持的指令集合内。 */
    const val UNSUPPORTED_CMD: String = "unsupportedCmd"

    /** 无屏时收到输入指令（`input.md` §8）—— PC 建屏后重试即可。 */
    const val NO_SCREEN: String = "noScreen"

    /** 输入载荷与 `action` 对不上（缺载荷 / 触点 id 变化与 action 矛盾）。 */
    const val BAD_INPUT: String = "badInput"

    /** 注入被系统策略禁止（缺 `INJECT_EVENTS`）—— 子系统级，重试无意义。 */
    const val INPUT_DENIED: String = "inputDenied"

    /** 单次注入失败（返回 `false` / 抛异常）—— 可恢复级。 */
    const val INPUT_FAILED: String = "inputFailed"
}
