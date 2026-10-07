package com.xiaowine.wcreen.agent.runtime

import android.system.Os
import android.system.OsConstants
import android.system.StructPollfd
import com.google.gson.JsonObject
import com.xiaowine.wcreen.agent.encode.CodecCaps
import com.xiaowine.wcreen.agent.input.InputCmd
import com.xiaowine.wcreen.agent.proto.AppliedMsg
import com.xiaowine.wcreen.agent.proto.ApplyScreenArgs
import com.xiaowine.wcreen.agent.proto.CapsRequestMsg
import com.xiaowine.wcreen.agent.proto.Cmd
import com.xiaowine.wcreen.agent.proto.Codec
import com.xiaowine.wcreen.agent.proto.CommandAckMsg
import com.xiaowine.wcreen.agent.proto.CommandMsg
import com.xiaowine.wcreen.agent.proto.ErrorCode
import com.xiaowine.wcreen.agent.proto.ErrorMsg
import com.xiaowine.wcreen.agent.proto.EventMsg
import com.xiaowine.wcreen.agent.proto.HelloAckMsg
import com.xiaowine.wcreen.agent.proto.HelloMsg
import com.xiaowine.wcreen.agent.proto.HeartbeatMsg
import com.xiaowine.wcreen.agent.proto.InputArgs
import com.xiaowine.wcreen.agent.proto.ResultMsg
import com.xiaowine.wcreen.agent.proto.Timing
import com.xiaowine.wcreen.agent.proto.Wire
import com.xiaowine.wcreen.agent.transport.Conn
import com.xiaowine.wcreen.agent.transport.Handshake

/**
 * 每路的唯一读者。
 *
 * 判活看**读超时**：`T_dead` 内没收到对端任何消息即判死（不是"没收到心跳"）。
 * 有 fd 的传输走 `Os.poll`（超时就是判据本身，不需要 `lastRecv` 记账）；没有 fd 的走
 * socket 自带的读超时 —— **判据相同，机制不同**。
 *
 * 分发：链路指令投 `link`，会话级指令投 `main`，心跳 / 握手应答 / 能力表本线程处理。
 *
 * @param routeId 路序号，只用于日志
 * @param conn 本路的连接
 * @param ln 日志出口
 * @param shutdown 全局停止标志
 * @param coordinator 协调者；`stopAgent` 由它接手
 * @param caps 设备编码能力；能力表请求直接用它应答
 * @param outbox 把一条待发消息投给本路唯一的写者
 * @param linkCmd 把一条链路指令投给 `link`
 * @param inputCmd 把一条输入事件 / 目标屏变化投给 `input`
 * @param onConnectionEnd 连接结束时回调（由本线程的 finally 触发）
 */
internal class ReaderLoop(
    private val routeId: Int,
    private val conn: Conn,
    private val ln: Ln,
    private val shutdown: Shutdown,
    private val coordinator: Coordinator,
    private val caps: CodecCaps,
    private val outbox: (Wire.Frame) -> Boolean,
    private val linkCmd: (LinkLoop.LinkCmd) -> Boolean,
    private val inputCmd: (InputCmd) -> Unit,
    private val onConnectionEnd: () -> Unit,
) : Thread("reader") {

    /** `Os.poll` 的描述符数组 —— 只在本线程用；传输不提供 fd 时为 null。 */
    private val pollfds: Array<StructPollfd>? = conn.fd?.let { fd ->
        arrayOf(
            StructPollfd().also {
                it.fd = fd
                it.events = OsConstants.POLLIN.toShort()
            },
        )
    }

    init {
        // 没有 fd 的传输（TCP）靠 socket 自带的读超时判活：判据相同 ——
        // T_dead 内读不到任何字节即判死。
        if (pollfds == null) conn.setReadTimeout(Timing.T_DEAD_MS.toInt())
    }

    override fun run() {
        try {
            if (handshake()) readLoop()
        } catch (t: Throwable) {
            // R2：线程意外退出一律 fatal —— 不做"部分存活"模式（link 死了帧通路就断了）。
            coordinator.reportFatal(
                Fail(FailLevel.FATAL, FailCode.UNEXPECTED, "路 #$routeId 的 reader 异常退出", t),
            )
        } finally {
            onConnectionEnd()
        }
    }

    /**
     * 回一条指令回报 —— `link` 也用它（建屏是异步的，回报在那里产生）。
     *
     * `applied` 的两个位置**各司其职**，不要重复发：
     * - **成功** ⇒ 走 `result`（含 `streamId` + 全量生效值）；
     * - **失败但部分生效** ⇒ 走**顶层** `applied`（此时没有 `result`），
     *   让"码率生效了但尺寸没改成"这类情况不丢信息。
     *
     * @param requestId 原样带回的关联号
     * @param ok 是否成功
     * @param fail 失败时的失败对象；成功给 null
     * @param applied 每个参数的实际生效值；没有就给 null
     */
    fun replyAck(requestId: Int, ok: Boolean, fail: Fail?, applied: AppliedMsg?) {
        val error = fail?.let { ErrorMsg(errorClass(it.level), it.code, it.message) }
        val result = if (ok && applied != null) ResultMsg(applied.streamId, applied) else null
        val partial = if (!ok) applied else null
        send(Wire.TYPE_COMMAND_ACK, CommandAckMsg(requestId, ok, result, partial, error))
    }

    /**
     * 发一条主动上报 —— 只有 **agent 单方面改变的事实** 才报。
     *
     * @param event 事件名（`screenReleased` / `subsystemUnavailable` / `fatal`）
     * @param reason 原因，诊断用
     * @param subsystem 出问题的子系统名；**只有 `subsystemUnavailable` 带它**
     */
    fun reportEvent(event: String, reason: String, subsystem: String? = null) {
        send(Wire.TYPE_EVENT, EventMsg(event, reason, subsystem))
    }

    /**
     * 握手：只做版本校验。
     *
     * PC 先说话；从连接到握手完成沿用 `T_dead`，不另设值 —— 两者都是
     * 「连上了但收不到消息」，语义与处置一致。
     *
     * @return true 表示握手通过，可以进读循环
     */
    private fun handshake(): Boolean {
        val frame = nextFrame() ?: return false
        if (frame.type != Wire.TYPE_HELLO) {
            ln.w("路 #$routeId 握手首包类型是 0x%02x，期望 0x%02x".format(frame.type, Wire.TYPE_HELLO))
            return false
        }

        val hello = when (val r = Codec.decode(frame.type, frame.payload, HelloMsg::class.java)) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> {
                ln.w("路 #$routeId ${r.fail.message}")
                return false
            }
        }

        // 有硬编才敢报 ready —— 没有编码器时后续 applyScreen 一律会被拒。
        val ack = HelloAckMsg(Handshake.PROTOCOL_VERSION, HelloAckMsg.STATE_READY)
        val check = Handshake.checkVersion(hello.version)
        // 两种情况都回报（含双方版本号）—— 不匹配时 PC 需要知道设备上是哪个版本。
        send(Wire.TYPE_HELLO_ACK, ack)
        if (check is Outcome.Err) {
            ln.w("路 #$routeId ${check.fail.message}")
            return false
        }
        ln.i("路 #$routeId 握手完成（协议版本 ${Handshake.PROTOCOL_VERSION}）")
        return true
    }

    /** 读循环：判活 → 读一帧 → 分发。 */
    private fun readLoop() {
        while (!shutdown.stopRequested) {
            val frame = nextFrame() ?: return
            when (frame.type) {
                // 心跳也要过 CRC：它的负载虽是空对象，却是"失步检测"最主要的锚点 ——
                // 视频帧不带 CRC，失步只能靠控制消息发现，而心跳是其中频率最高的一条。
                Wire.TYPE_HEARTBEAT -> {
                    val decoded = Codec.decode(frame.type, frame.payload, HeartbeatMsg::class.java)
                    if (decoded is Outcome.Err) {
                        ln.w("路 #$routeId ${decoded.fail.message} ⇒ 协议级错误，断连")
                        return
                    }
                    ln.v("路 #$routeId 收到心跳")
                }

                Wire.TYPE_CAPS_REQUEST -> handleCapsRequest(frame)

                Wire.TYPE_COMMAND -> handleCommand(frame)

                else -> {
                    // 版本精确匹配 ⇒ 不可能收到别的版本的帧；收到就是协议错误。
                    ln.w("路 #$routeId 收到本实现不处理的类型 0x%02x ⇒ 协议级错误，断连".format(frame.type))
                    return
                }
            }
        }
    }

    /**
     * 判活后读一帧。
     *
     * @return 帧；null 表示本线程应当退出（判死 / 对端关闭 / 协议错误 / 全局停止）
     */
    private fun nextFrame(): Wire.Frame? {
        if (!awaitReadable()) return null
        return when (val r = Wire.readFrame(conn.input)) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> {
                ln.i("路 #$routeId ${r.fail.message}")
                null
            }
        }
    }

    /**
     * 等对端有数据；`T_dead` 内一直没有 ⇒ 判死。
     *
     * 有 fd 的传输走 `Os.poll`；没有 fd 的传输已经在构造时把 `T_dead` 交给 socket 自身的读超时，
     * 这里直接放行，由读抛出超时异常来判死。
     *
     * @return false 表示本线程应当退出
     */
    private fun awaitReadable(): Boolean {
        val fds = pollfds ?: return true
        val ready = try {
            Os.poll(fds, Timing.T_DEAD_MS.toInt())
        } catch (e: Exception) {
            ln.w("路 #$routeId poll 失败：${e.message}")
            return false
        }
        if (ready == 0) {
            ln.i("路 #$routeId ${Timing.T_DEAD_MS} ms 内没有任何消息 ⇒ 判 PC 掉线")
            return false
        }
        return true
    }

    /**
     * 应答能力表请求。
     *
     * 表是**启动时构建一次**的设备级静态量，全进程不变 ⇒ 任何时候请求都返回同一份，
     * 没有 `forceRefresh`。
     *
     * @param frame 类型为 [Wire.TYPE_CAPS_REQUEST] 的帧
     */
    private fun handleCapsRequest(frame: Wire.Frame) {
        val request = when (val r = Codec.decode(frame.type, frame.payload, CapsRequestMsg::class.java)) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> {
                ln.w("路 #$routeId ${r.fail.message}")
                return
            }
        }
        send(Wire.TYPE_CAPS, caps.message(request.requestId))
        ln.d("路 #$routeId 已应答能力表（requestId=${request.requestId}）")
    }

    /**
     * 处理一条指令。
     *
     * @param frame 类型为 [Wire.TYPE_COMMAND] 的帧
     */
    private fun handleCommand(frame: Wire.Frame) {
        val cmd = when (val r = Codec.decode(frame.type, frame.payload, CommandMsg::class.java)) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> {
                ln.w("路 #$routeId ${r.fail.message}")
                return
            }
        }

        when (cmd.cmd) {
            Cmd.STOP_AGENT -> {
                ln.i("路 #$routeId 收到 stopAgent（requestId=${cmd.requestId}）⇒ 全局收尾")
                replyAck(cmd.requestId, ok = true, fail = null, applied = null)
                coordinator.requestStop(ExitCode.OK)
            }

            Cmd.APPLY_SCREEN -> {
                val args = decodeArgs(cmd.payload, ApplyScreenArgs::class.java) { ApplyScreenArgs() }
                    ?: return
                handleApplyScreen(args, cmd.requestId)
            }

            Cmd.REQUEST_SYNC_FRAME -> {
                linkCmd(LinkLoop.LinkCmd.RequestSync)
                replyAck(cmd.requestId, ok = true, fail = null, applied = null)
            }

            Cmd.END_SESSION -> {
                ln.i("路 #$routeId 收到 endSession（requestId=${cmd.requestId}）⇒ 拆屏但继续监听")
                linkCmd(LinkLoop.LinkCmd.Release)
                replyAck(cmd.requestId, ok = true, fail = null, applied = null)
            }

            Cmd.INPUT -> {
                val args = decodeArgs(cmd.payload, InputArgs::class.java) { InputArgs() } ?: return
                handleInput(args, cmd.requestId)
            }

            else -> {
                ln.w("路 #$routeId 收到本实现不支持的指令 '${cmd.cmd}'")
                replyAck(
                    cmd.requestId,
                    ok = false,
                    fail = Fail(
                        FailLevel.STREAM,
                        ErrorCode.UNSUPPORTED_CMD,
                        "指令 '${cmd.cmd}' 不在本实现支持的指令集合内",
                    ),
                    applied = null,
                )
            }
        }
    }

    /**
     * 解信封里的载荷 —— 第二段解析。
     *
     * 指令的字段全部落在信封的 `payload` 里；`payload` 缺省表示空载荷，用 [empty] 给的默认结构体。
     *
     * @param payload 信封里取出的载荷对象
     * @param clazz 载荷结构体类型
     * @param empty 空载荷时构造的默认结构体
     * @return 载荷；null 表示解析失败（已记日志），调用方直接返回
     */
    private fun <T : Any> decodeArgs(payload: JsonObject?, clazz: Class<T>, empty: () -> T): T? =
        when (val r = Codec.decodePayload(payload, clazz, empty)) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> {
                ln.w("路 #$routeId ${r.fail.message}")
                null
            }
        }

    /**
     * 处理「申请屏幕」—— 真正的建屏在 `link` 上做，这里只把载荷转发出去。
     *
     * @param args 已解码的载荷
     * @param requestId 信封里的关联号；建屏是异步的，回报由 `link` 产生
     */
    private fun handleApplyScreen(args: ApplyScreenArgs, requestId: Int) {
        // 屏名缺省 = WCReen-<路号>，便于 dumpsys display 排查与多路区分。
        val posted = linkCmd(
            LinkLoop.LinkCmd.Apply(args.params, requestId, "WCReen-$routeId"),
        )
        if (!posted) {
            replyAck(
                requestId,
                ok = false,
                fail = Fail(FailLevel.RECOVERABLE, FailCode.BAD_MESSAGE, "链路指令队列已满，请重试"),
                applied = null,
            )
        }
    }

    /**
     * 处理输入事件 —— 只入队，**本线程不回报**。
     *
     * 回报由 `input` 线程按"有没有真的注入成功"决定，且**只报失败**（`input.md` §7）：
     * 成功、被合并、被丢弃都静默 —— 输入是高频流，逐条回 ack 会把协议开销翻倍，
     * 而它没有任何信息量。
     *
     * @param args 已解码的载荷
     * @param requestId 信封里的关联号；随事件一起投给 `input`，失败时原样回报
     */
    private fun handleInput(args: InputArgs, requestId: Int) {
        inputCmd(InputCmd.Event(args, requestId))
    }

    /**
     * 把一条控制消息交给本路唯一的写者。
     *
     * @param type 帧类型
     * @param msg 消息 DTO
     */
    private fun send(type: Int, msg: Any) {
        if (!outbox(Wire.Frame(type, Codec.encode(type, msg)))) {
            // 队列满只可能是写者卡住；此时连接已不可用，由 T_dead 收口。
            ln.w("路 #$routeId 控制队列已满，丢弃一条待发消息（类型 0x%02x）".format(type))
        }
    }

    /**
     * 失败级别 → 协议里的 `class` 字符串。
     *
     * @param level 失败级别
     * @return `fatal` / `subsystem` / `stream` / `recoverable`
     */
    private fun errorClass(level: FailLevel): String = when (level) {
        FailLevel.FATAL -> "fatal"
        FailLevel.SUBSYSTEM -> "subsystem"
        FailLevel.STREAM -> "stream"
        FailLevel.RECOVERABLE -> "recoverable"
    }
}
