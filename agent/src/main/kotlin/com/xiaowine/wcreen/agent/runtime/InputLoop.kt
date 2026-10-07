package com.xiaowine.wcreen.agent.runtime

import com.xiaowine.wcreen.agent.input.EventBuilder
import com.xiaowine.wcreen.agent.input.InputCmd
import com.xiaowine.wcreen.agent.input.InputQueue
import com.xiaowine.wcreen.agent.input.PointerTracker
import com.xiaowine.wcreen.agent.proto.ErrorCode
import com.xiaowine.wcreen.agent.proto.InputArgs
import com.xiaowine.wcreen.agent.wrappers.InputInjector

/**
 * 每路的输入注入线程（`docs/spec/agent/input.md` §5）。
 *
 * 为什么必须是**独立的一条线程**：注入一次 ~3 ms，而 `slow` 上的一次枚举是百毫秒级
 * —— 共用会让拖动卡住（`runtime-threads.md` §2 的线程表把这条列为"不与 `slow` 共用"）。
 *
 * 它靠**合并**而不是吞吐来保证跟手：队列里永远只剩最新状态，最坏延迟 = 一次注入，
 * 与 PC 送多快无关（`input/InputQueue.kt`）。
 *
 * 目标屏由 `link` 经队列投递（`InputCmd.ScreenChanged`）而不是共享变量：
 * 屏的 owner 是 `link`，本线程不该去读它的状态（`code.md` §6）。
 *
 * @param routeId 路序号，只用于日志
 * @param ln 日志出口
 * @param shutdown 全局停止标志
 * @param stopSignal 本线程的退出信号（收尾时由 `Route` 置位）
 * @param coordinator 协调者；本线程意外退出要报 fatal
 * @param injector 注入器（进程级共享，构造时已解析完毕）
 * @param queue 注入队列；生产者是 `reader` 与 `link`
 * @param failAck 失败回报出口（成功**不回**，见 `input.md` §7）
 * @param reportEvent 主动上报出口；第三参是子系统名（`subsystemUnavailable` 用）
 */
internal class InputLoop(
    private val routeId: Int,
    private val ln: Ln,
    private val shutdown: Shutdown,
    private val stopSignal: Signal,
    private val coordinator: Coordinator,
    private val injector: InputInjector.Injector,
    private val queue: InputQueue,
    private val failAck: (requestId: Int, fail: Fail) -> Unit,
    private val reportEvent: (event: String, reason: String, subsystem: String?) -> Unit,
) : Thread("input") {

    /** 触点跟踪器 —— 推导 `pointerIndex` 与 `downTime`；单线程，无锁。 */
    private val tracker: PointerTracker = PointerTracker()

    /** 当前目标屏；[NONE] 表示无屏（此时拒绝输入并回报失败，`input.md` §8）。 */
    private var targetDisplayId: Int = NONE

    /** 是否已判定输入子系统不可用 —— 只**首次**上报，之后各指令回报同一原因。 */
    private var blocked: Boolean = false

    /**
     * 投一条消息给本线程（`reader` 投事件、`link` 投屏变化）。
     *
     * @param cmd 投递的消息
     */
    fun post(cmd: InputCmd) {
        queue.offer(cmd)
    }

    override fun run() {
        try {
            while (!shutdown.stopRequested && !stopSignal.isRaised) {
                val cmd = queue.take(POLL_TIMEOUT_MS) ?: continue
                when (cmd) {
                    is InputCmd.ScreenChanged -> {
                        targetDisplayId = cmd.displayId
                        ln.d("路 #$routeId 的输入目标屏：${if (cmd.displayId > 0) cmd.displayId.toString() else "无"}")
                    }

                    is InputCmd.Event -> inject(cmd.msg, cmd.requestId)
                }
            }
        } catch (t: Throwable) {
            // R2：线程意外退出一律 fatal —— 不做"部分存活"模式（input 死了输入就永久不动了）。
            coordinator.reportFatal(
                Fail(FailLevel.FATAL, FailCode.UNEXPECTED, "路 #$routeId 的 input 异常退出", t),
            )
        }
    }

    /**
     * 注入一条事件。
     *
     * 失败分三档（`input.md` §7 / §8）：
     * - 无屏 ⇒ 可恢复级 + `noScreen`（PC 在建屏后重试即可）；
     * - 载荷不合法 ⇒ 可恢复级 + `badInput`（PC 侧 bug）；
     * - 注入被系统策略禁止 ⇒ **子系统级** + `inputDenied`，并**首次**上报后拒绝后续输入。
     *
     * @param msg 已解码的输入载荷
     * @param requestId 信封里的关联号，失败回报时原样带回
     */
    private fun inject(msg: InputArgs, requestId: Int) {
        if (blocked) {
            failAck(
                requestId,
                Fail(FailLevel.SUBSYSTEM, ErrorCode.INPUT_DENIED, "输入子系统不可用（已上报）"),
            )
            return
        }
        if (targetDisplayId <= 0) {
            failAck(
                requestId,
                Fail(FailLevel.RECOVERABLE, ErrorCode.NO_SCREEN, "无屏，无法注入输入"),
            )
            return
        }

        val event = when (val r = EventBuilder.build(msg, tracker)) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> {
                ln.d("路 #$routeId 输入载荷不合法：${r.fail.message}")
                failAck(requestId, r.fail)
                return
            }
        }

        when (val r = injector.inject(event, targetDisplayId)) {
            // 成功静默：输入是高频流，逐条回 ack 会把协议开销翻倍，而它没有任何信息量。
            is Outcome.Ok -> Unit

            is Outcome.Err -> {
                if (r.fail.level == FailLevel.SUBSYSTEM) blocked = true
                ln.w("路 #$routeId 注入失败：${r.fail}")
                failAck(requestId, r.fail)
                if (blocked) reportEvent(EVENT_SUBSYSTEM_UNAVAILABLE, r.fail.message, SUBSYSTEM_INPUT)
            }
        }
    }

    /** 常量。 */
    companion object {

        /** 无屏时的目标屏取值。 */
        private const val NONE: Int = -1

        /** 队列取超时 —— 靠"醒来看停止标志"退出，与全进程收尾手法一致。 */
        private const val POLL_TIMEOUT_MS: Long = 250

        /** 子系统不可用的上报事件名。 */
        private const val EVENT_SUBSYSTEM_UNAVAILABLE: String = "subsystemUnavailable"

        /** 本子系统名（上报时用）。 */
        private const val SUBSYSTEM_INPUT: String = "input"
    }
}
