package com.xiaowine.wcreen.agent.runtime

/**
 * 失败级别 —— 按「谁负责善后」分四级（`runtime-failure.md` §1）。
 *
 * 级别随失败对象走，**善后由收方按级别决定，不在抛出处决定**。
 */
internal enum class FailLevel {

    /** 进程无法继续正确运行 ⇒ 全局收尾 + `System.exit(非 0)`。 */
    FATAL,

    /** 能力 / 机制缺失、重试无意义 ⇒ 标记该子系统不可用，拒绝后续相关指令。 */
    SUBSYSTEM,

    /** 该路（该屏 / 该连接）不可用，其余健康 ⇒ 回收该路资源 + 上报。 */
    STREAM,

    /** 本次操作失败、重试可能成功 ⇒ 只上报，不动任何状态。重试由 PC 负责。 */
    RECOVERABLE,
}

/**
 * 一次失败的描述。
 *
 * @property level 失败级别，决定谁负责善后
 * @property code 机器可读的稳定标识；会上线的取值登记在 `proto.ErrorCode`
 * @property message 给人看的可诊断说明
 * @property cause 原始异常（跨平台边界捕获的）；纯逻辑失败为 null
 */
internal class Fail(
    val level: FailLevel,
    val code: String,
    val message: String,
    val cause: Throwable? = null,
) {
    override fun toString(): String = "[$level/$code] $message"
}

/**
 * 操作结果。
 *
 * 可预期失败走这里，**不用异常做控制流**（`code.md` §3）。异常只在跨平台边界
 * （Android API / socket 抛出的）被捕获，并立即转成 [Fail]。
 */
internal sealed class Outcome<out T> {

    /** 成功，并携带结果值。 */
    class Ok<out T>(val value: T) : Outcome<T>()

    /** 失败，携带带级别的 [Fail]。 */
    class Err(val fail: Fail) : Outcome<Nothing>()
}

/** 成功时取出结果值，失败时返回 null。 */
internal fun <T> Outcome<T>.valueOrNull(): T? = (this as? Outcome.Ok)?.value

/** 失败时取出失败对象，成功时返回 null。 */
internal fun Outcome<*>.failOrNull(): Fail? = (this as? Outcome.Err)?.fail

/** 进程退出码（`runtime-failure.md` §4）。 */
internal object ExitCode {

    /** 正常结束 —— 上游显式停止 / 全部组件正常结束。 */
    const val OK: Int = 0

    /** 运行中致命错误。 */
    const val RUNTIME_FATAL: Int = 1

    /** 启动失败 —— 进程引导 / 参数非法 / 监听地址被占用。 */
    const val STARTUP_FAILURE: Int = 2
}

/**
 * 纯进程内的失败码 —— 不会出现在协议里。
 *
 * 会上线的错误码在 `proto.ErrorCode`；分开放是因为前者是给 PC 看的契约，
 * 后者只是日志里的定位线索。
 */
internal object FailCode {

    /** CLI 参数非法。 */
    const val BAD_CLI: String = "badCli"

    /** 监听地址被占用（重复实例 / 端口冲突）。 */
    const val ADDR_IN_USE: String = "addrInUse"

    /** 隐藏 API 成员解析失败（启动自检）。 */
    const val WRAPPER_MISSING: String = "wrapperMissing"

    /** 进程引导失败。 */
    const val BOOTSTRAP: String = "bootstrap"

    /** 帧头 / 长度非法。 */
    const val BAD_FRAME: String = "badFrame"

    /** 控制消息 CRC 不匹配。 */
    const val BAD_CRC: String = "badCrc"

    /** 控制消息 JSON 非法。 */
    const val BAD_JSON: String = "badJson"

    /** 消息类型或语义与当前状态不符。 */
    const val BAD_MESSAGE: String = "badMessage"

    /** 对端关闭连接。 */
    const val PEER_CLOSED: String = "peerClosed"

    /** 判活超时：`T_dead` 内没有收到对端任何消息。 */
    const val TIMEOUT: String = "timeout"

    /** 通用 IO 失败。 */
    const val IO: String = "io"

    /** 兜底：未分类的异常。 */
    const val UNEXPECTED: String = "unexpected"

    /** 建屏被系统拒绝（资源类，可恢复）。 */
    const val SCREEN_CREATE: String = "screenCreate"

    /** 建屏后回读 flags 与请求不符（平台没照做 ⇒ 致命）。 */
    const val SCREEN_VERIFY: String = "screenVerify"

    /** 没有可用的硬编编码器。 */
    const val NO_HW_ENCODER: String = "noHwEncoder"

    /** 编码器 configure / start 失败。 */
    const val ENCODER_OPEN: String = "encoderOpen"

    /** 编码器参数回读不符（致命）。 */
    const val ENCODER_VERIFY: String = "encoderVerify"

    /** 运行中编码器抛 `CodecException`。 */
    const val ENCODER_FAILED: String = "encoderFailed"

    /** CDM 关联缺失且建不出来（虚拟设备的硬前置）。 */
    const val ASSOCIATION: String = "association"
}
