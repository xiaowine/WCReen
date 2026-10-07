package com.xiaowine.wcreen.agent.runtime

import com.xiaowine.wcreen.agent.display.Screen
import com.xiaowine.wcreen.agent.encode.CodecCaps
import com.xiaowine.wcreen.agent.encode.EncoderParams
import com.xiaowine.wcreen.agent.encode.VideoEncoder
import com.xiaowine.wcreen.agent.input.InputCmd
import com.xiaowine.wcreen.agent.proto.AppliedMsg
import com.xiaowine.wcreen.agent.proto.ScreenParamsMsg
import java.nio.ByteBuffer
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit

/**
 * 每路的显示链路 —— **屏 + 编码器焊成一个状态机**（`docs/spec/agent/runtime-lifecycle.md` §2）。
 *
 * 为什么合成一个：零拷贝把屏的输出面与编码器的输入面焊死；重建五步与收尾顺序**全部成对**。
 * 分开管只会制造"屏在而编码器没了"这种无意义中间态。
 *
 * 对外只有 3 个状态（编码器的"未配置/已启动/已释放"是链路内部细节，不单独暴露）：
 *
 * | 状态 | 含义 |
 * |---|---|
 * | **无屏** | 无 `VirtualDisplay`、无编码器 |
 * | **就绪** | 屏 + 编码器 + 输出面已绑 |
 * | **冻结** | 屏与编码器都在，`setSurface(null)` 解绑了输出面 |
 *
 * **owner 纪律**：`MediaCodec` 的 `dequeue` 在这里、`releaseOutputBuffer` 在 `sender` —— 这是
 * 全模型唯一的共享点，不加锁（已实测跨线程 release 安全）。
 *
 * @param routeId 路序号，只用于日志
 * @param ln 日志出口
 * @param shutdown 全局停止标志
 * @param stopSignal 本路的链路停止信号（连接断了就停，免得留下永久空转的线程）
 * @param screen 本路的屏
 * @param encoder 本路的编码器
 * @param caps 设备编码能力
 * @param defaultWidth 缺省宽（主屏分辨率）
 * @param defaultHeight 缺省高（主屏分辨率）
 * @param defaultDpi 缺省密度（主屏 densityDpi）
 * @param frameQueue 帧队列；本线程**入队**，`sender` 出队
 * @param ack 指令回报的出口（`applyScreen` 的回报由本线程产生）
 */
internal class LinkLoop(
    private val routeId: Int,
    private val ln: Ln,
    private val shutdown: Shutdown,
    private val stopSignal: Signal,
    private val screen: Screen,
    private val encoder: VideoEncoder,
    private val caps: CodecCaps,
    private val defaultWidth: Int,
    private val defaultHeight: Int,
    private val defaultDpi: Int,
    private val frameQueue: ArrayBlockingQueue<PendingFrame>,
    private val ack: (requestId: Int, ok: Boolean, fail: Fail?, applied: AppliedMsg?) -> Unit,
) : Thread("link") {

    /** 指令队列；只有 `reader` / `main` 能投。 */
    private val cmdQueue: ArrayBlockingQueue<LinkCmd> = ArrayBlockingQueue(CMD_QUEUE_CAPACITY)

    /** 上一次请求 IDR 的时刻（毫秒）—— 限流用。 */
    private var lastIdrRequestMs: Long = 0

    /** 本路屏的标识 —— agent 分配，PC 记忆。 */
    private var streamId: Int = 0

    override fun run() {
        try {
            while (!shutdown.stopRequested && !stopSignal.isRaised) {
                drainCommands()
                pumpEncoder()
            }
        } catch (t: Throwable) {
            // R2：线程意外退出一律 fatal —— 不做"部分存活"模式。
            coordinatorFatal(t)
        } finally {
            // 收尾硬顺序：先屏后编码器（反了会让编码器的输入面在屏仍渲染时失效）。
            screen.release()
            encoder.release()
        }
    }

    /**
     * 投一条链路指令。
     *
     * @param cmd 指令
     * @return false 表示队列已满（链路卡住）
     */
    fun post(cmd: LinkCmd): Boolean = cmdQueue.offer(cmd)

    /** 处理完手头所有指令。 */
    private fun drainCommands() {
        while (true) {
            val cmd = cmdQueue.poll() ?: return
            when (cmd) {
                is LinkCmd.Apply -> applyScreen(cmd)
                LinkCmd.Release -> releaseLink()
                LinkCmd.RequestSync -> refresh()
            }
        }
    }

    /**
     * 取一帧编码输出并尝试入队。
     *
     * 没有编码器时用 `cmdQueue` 的短超时当节拍 —— 既不空转，指令也能及时被处理。
     */
    private fun pumpEncoder() {
        if (!encoder.isRunning) {
            val cmd = try {
                cmdQueue.poll(IDLE_POLL_MS, TimeUnit.MILLISECONDS)
            } catch (e: InterruptedException) {
                return
            }
            if (cmd != null) when (cmd) {
                is LinkCmd.Apply -> applyScreen(cmd)
                LinkCmd.Release -> releaseLink()
                LinkCmd.RequestSync -> refresh()
            }
            return
        }

        when (val polled = encoder.poll(DEQUEUE_TIMEOUT_US)) {
            is VideoEncoder.Poll.Idle -> Unit

            is VideoEncoder.Poll.Failed -> {
                ln.w("路 #$routeId ${polled.fail.message} ⇒ 拆屏")
                releaseLink()
                // 拆屏要**主动上报**：PC 不知道这件事，且它是"agent 单方面改变的事实"。
                reportEvent("screenReleased", polled.fail.code)
            }

            is VideoEncoder.Poll.Frame -> {
                val pending = PendingFrame(
                    index = polled.index,
                    keyFrame = polled.keyFrame,
                    ptsUs = polled.ptsUs,
                    data = polled.data,
                    enqueueNanos = System.nanoTime(),
                )
                if (!frameQueue.offer(pending)) {
                    // 队列满 ⇒ 切断背压链条：**立即 release**，否则输出池会耗尽、屏上的 App 被拖死。
                    encoder.releaseOutputBuffer(polled.index)
                    requestIdr()
                }
            }
        }
    }

    /**
     * 处理「申请屏幕」—— 显示链路状态机的唯一入口。
     *
     * 转移规则（`runtime-lifecycle.md` §2.2）：
     * - **无屏** ⇒ 建屏；
     * - **就绪 / 冻结 + 参数未变** ⇒ 解冻（换面 + 请求同步帧）；
     * - **就绪 / 冻结 + 参数变** ⇒ 重建编码器（必要时连屏尺寸一起改）。
     *
     * @param cmd 请求
     */
    private fun applyScreen(cmd: LinkCmd.Apply) {
        val resolved = when (val r = EncoderParams.resolve(
            params = cmd.params,
            caps = caps,
            screenWidth = defaultWidth,
            screenHeight = defaultHeight,
            screenDpi = defaultDpi,
            screenName = cmd.screenName,
        )) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> {
                ln.w("路 #$routeId 申请屏幕被拒：${r.fail.message}")
                ack(cmd.requestId, false, r.fail, null)
                return
            }
        }

        val applied = AppliedMsg(
            streamId = streamId,
            width = resolved.width,
            height = resolved.height,
            refreshRate = resolved.refreshRate,
            dpi = resolved.dpi,
            bitrate = resolved.bitrate,
            bitrateMode = resolved.bitrateModeName,
            codec = resolved.codecName,
            codecName = resolved.encoderName,
            hardware = resolved.hardware,
            bitDepth = CodecCaps.BIT_DEPTH_8,
            profile = resolved.profileName,
            colorStandard = CodecCaps.colorStandardName(resolved.colorStandard),
            colorRange = CodecCaps.colorRangeName(resolved.colorRange),
            colorTransfer = CodecCaps.colorTransferName(resolved.colorTransfer),
            showSystemDecorations = resolved.showSystemDecorations,
            alwaysUnlocked = resolved.alwaysUnlocked,
            // 新路径**不请求** `OWN_DISPLAY_GROUP`，平台也不会给虚拟设备的屏加它
            // —— 回读校验把该位写进掩码（出现即致命）⇒ 这里的 false 是被校验保证过的。
            ownDisplayGroup = false,
            screenName = resolved.screenName,
        )

        // 参数完全一致 ⇒ 只解冻，不重建（冻结方案 A 的价值就在这里）。
        val current = screen.currentRequest()
        val unchanged = current != null &&
            current.width == resolved.width &&
            current.height == resolved.height &&
            current.dpi == resolved.dpi &&
            current.name == resolved.screenName &&
            current.showSystemDecorations == resolved.showSystemDecorations &&
            current.alwaysUnlocked == resolved.alwaysUnlocked &&
            encoder.isRunning

        if (unchanged) {
            unfreeze()
            if (streamId == 0) streamId = nextStreamId()
            applied.streamId = streamId
            ln.i("路 #$routeId 参数未变 ⇒ 解冻（streamId=$streamId）")
            publishScreen()
            ack(cmd.requestId, true, null, applied)
            return
        }

        // 重建：① 解绑 ② 释放旧编码器 ③ 新建 ④ 绑新面 ⑤ 请求同步帧。
        // ① 必须在 ② 之前 —— 否则屏还在往一个即将失效的面上写。
        if (encoder.isRunning) {
            screen.setSurface(null)
            encoder.release()
        }

        val newSurface = when (val r = encoder.open(resolved)) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> {
                ln.w("路 #$routeId 建屏失败：${r.fail.message}")
                // 编码器没了 ⇒ 屏也留不住（链路是焊死的），一并释放。
                screen.release()
                ack(cmd.requestId, false, r.fail, null)
                return
            }
        }

        if (!screen.exists) {
            when (val r = screen.create(
                name = resolved.screenName,
                width = resolved.width,
                height = resolved.height,
                dpi = resolved.dpi,
                showSystemDecorations = resolved.showSystemDecorations,
                alwaysUnlocked = resolved.alwaysUnlocked,
                surface = newSurface,
            )) {
                is Outcome.Ok -> Unit
                is Outcome.Err -> {
                    encoder.release()
                    ack(cmd.requestId, false, r.fail, null)
                    return
                }
            }
        } else {
            // 尺寸变了必须成对改：屏尺寸 = 编码尺寸，不允许出现尺寸不匹配的窗口。
            when (val r = screen.resize(resolved.width, resolved.height, resolved.dpi)) {
                is Outcome.Ok -> Unit
                is Outcome.Err -> {
                    ln.w("路 #$routeId ${r.fail.message}")
                    ack(cmd.requestId, false, r.fail, null)
                    return
                }
            }
            screen.setSurface(newSurface)
        }

        // ⚠️ 必须请求同步帧：换面会触发一次渲染，但不请求的话首帧不是 IDR ⇒ PC 黑屏。
        encoder.requestSyncFrame()

        if (streamId == 0) streamId = nextStreamId()
        applied.streamId = streamId
        ln.i("路 #$routeId 链路就绪（streamId=$streamId，displayId=${screen.displayId}）")
        publishScreen()
        ack(cmd.requestId, true, null, applied)
    }

    /** 解冻：换面 + 立刻请求同步帧。换面本身会触发一次渲染。 */
    private fun unfreeze() {
        screen.setSurface(null)
        screen.setSurface(encoder.surface)
        encoder.requestSyncFrame()
    }

    /** 主动刷新（丢包重同步 / PC 解码器重启）—— 与解冻是同一条路径。 */
    private fun refresh() {
        if (!encoder.isRunning || !screen.exists) return
        unfreeze()
        ln.i("路 #$routeId 已主动刷新（换面 + 请求同步帧）")
    }

    /**
     * 发一条主动上报。
     *
     * @param event 事件名
     * @param reason 原因
     */
    private fun reportEvent(event: String, reason: String) {
        eventSink?.invoke(event, reason)
    }

    /**
     * 拆屏 —— 硬顺序：先屏后编码器。幂等。
     */
    private fun releaseLink() {
        if (!screen.exists && !encoder.isRunning) return
        screen.release()
        encoder.release()
        streamId = 0
        // 拆完要告诉 input：`Screen.displayId` 随释放变回 -1 ⇒ 这段自然表达"无屏"。
        publishScreen()
        ln.i("路 #$routeId 链路已拆除")
    }

    /**
     * 把当前目标屏告诉 `input` 线程。
     *
     * 必须在**回报 `applyScreen` 成功之前**发布：PC 一收到回报就会开始发输入事件，
     * 若它比这条发布先到，`input` 会以"无屏"把事件拒掉。
     */
    private fun publishScreen() {
        inputSink?.invoke(InputCmd.ScreenChanged(screen.displayId))
    }

    /**
     * 请求一个同步帧，**限流 ≥ 1s**。
     *
     * 为什么必须主动请求：静止屏上 `KEY_I_FRAME_INTERVAL` 按"编码的帧"计 ⇒ 周期 IDR 永远不来，
     * 一旦在静止时进入丢弃模式，PC 会**永久黑屏**。限流是因为 IDR 是最大的帧，请求它反而加剧拥塞。
     */
    private fun requestIdr() {
        if (!encoder.isRunning || !screen.exists) return
        val now = System.currentTimeMillis()
        if (now - lastIdrRequestMs < IDR_THROTTLE_MS) return
        lastIdrRequestMs = now
        unfreeze()
        ln.d("路 #$routeId 丢帧后请求 IDR")
    }

    /**
     * 上报致命失败。
     *
     * @param t 原始异常
     */
    private fun coordinatorFatal(t: Throwable) {
        ln.e("路 #$routeId 的 link 异常退出", t)
        fatalSink?.invoke(Fail(FailLevel.FATAL, FailCode.UNEXPECTED, "路 #$routeId 的 link 异常退出", t))
    }

    /** 致命通道 —— 由 `Route` 装配时注入，避免 link 反向依赖协调者。 */
    var fatalSink: ((Fail) -> Unit)? = null

    /** 主动上报通道 —— 同样由 `Route` 注入。 */
    var eventSink: ((event: String, reason: String) -> Unit)? = null

    /** 目标屏发布通道 —— 由 `Route` 注入，把"当前目标屏"投给 `input` 线程。 */
    var inputSink: ((InputCmd) -> Unit)? = null

    /** 分配下一个屏标识。 */
    private fun nextStreamId(): Int = ++streamIdCounter

    /** 链路指令。 */
    sealed class LinkCmd {

        /**
         * 申请屏幕。
         *
         * @property params PC 给的建屏参数
         * @property requestId 关联号
         * @property screenName 缺省屏名
         */
        class Apply(val params: ScreenParamsMsg, val requestId: Int, val screenName: String) : LinkCmd()

        /** 结束会话：拆屏但继续监听。 */
        object Release : LinkCmd()

        /** 请求同步帧。 */
        object RequestSync : LinkCmd()
    }

    /** 常量。 */
    companion object {

        /** 指令队列容量 —— 链路指令只有建屏 / 拆屏 / 请求同步三种，几条就够。 */
        private const val CMD_QUEUE_CAPACITY: Int = 8

        /** `dequeue` 短超时：指令能及时被处理，又不空转。 */
        private const val DEQUEUE_TIMEOUT_US: Long = 10_000

        /** 没有编码器时的空转节拍。 */
        private const val IDLE_POLL_MS: Long = 50

        /** 请求 IDR 的最小间隔 —— 防 IDR 风暴（IDR 是最大的帧，请求它反而加剧拥塞）。 */
        private const val IDR_THROTTLE_MS: Long = 1_000

        /** 屏标识分配器 —— 进程内单调递增。 */
        private var streamIdCounter: Int = 0
    }
}

/**
 * 队列里的一帧 —— **还没 release 的输出缓冲**。
 *
 * 每帧记**入队时刻**：`T_drop` 的判据是"等待时间"而不是"队列满"——
 * 实测真实链路下容量会被打满，但那不是 PC 跟不上，用容量当判据会误触发。
 *
 * @property index 编码器输出缓冲下标；写完必须 `releaseOutputBuffer(index)`
 * @property keyFrame 是否关键帧
 * @property ptsUs 呈现时间戳（微秒）
 * @property data 编码数据（Annex-B），只读
 * @property enqueueNanos 入队时刻（`System.nanoTime()`）
 */
internal class PendingFrame(
    val index: Int,
    val keyFrame: Boolean,
    val ptsUs: Long,
    val data: ByteBuffer,
    val enqueueNanos: Long,
)
