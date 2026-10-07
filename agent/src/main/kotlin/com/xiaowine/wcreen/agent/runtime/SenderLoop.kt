package com.xiaowine.wcreen.agent.runtime

import com.xiaowine.wcreen.agent.encode.VideoEncoder
import com.xiaowine.wcreen.agent.proto.FramePayload
import com.xiaowine.wcreen.agent.proto.Wire
import com.xiaowine.wcreen.agent.transport.Conn
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit

/**
 * 每路的唯一写者。
 *
 * 为什么必须独立成线程：心跳与控制应答也写这条 socket。若和帧通路共用一条线程，
 * 心跳就只能排队等帧、或加锁阻塞帧 —— 两种都违反「实时流不得被输出缓冲扣住」。
 * 独立之后网络抖动只波及它，编码器不受牵连。
 *
 * **丢弃策略**（`docs/spec/agent/transport-backpressure.md` §2/§3）：
 * - 判据是**等待时间** `T_drop`，**不是队列满** —— 实测真实链路下容量会被打满，
 *   但那不是 PC 跟不上，用容量当判据会误触发；
 * - 一旦丢，就**丢掉后续所有非 IDR，直到发出一个 IDR** —— H.264/HEVC 有参考关系，
 *   丢中间一个 P 帧会让后续直到下一个 IDR 全部花屏；
 * - 进入丢弃模式时**投一次**「请求 IDR」给 `link`（否则静止屏上周期 IDR 永远不来 ⇒ 永久黑屏）。
 *
 * @param routeId 路序号，只用于日志
 * @param conn 本路的连接
 * @param ln 日志出口
 * @param controlQueue 控制消息队列；**优先级高于帧**
 * @param frameQueue 帧队列；元素是**还没 release** 的输出缓冲
 * @param encoder 用于把缓冲还给编码器（全模型唯一的跨线程共享对象，不加锁）
 * @param requestIdr 请求同步帧的回调（走 `link`）
 * @param stopSignal 收尾信号 —— 与全局停止标志分开，因为本线程必须**最后**停
 * @param coordinator 协调者；异常退出时报 fatal
 */
internal class SenderLoop(
    private val routeId: Int,
    private val conn: Conn,
    private val ln: Ln,
    private val controlQueue: ArrayBlockingQueue<Wire.Frame>,
    private val frameQueue: ArrayBlockingQueue<PendingFrame>,
    private val encoder: VideoEncoder,
    private val requestIdr: () -> Unit,
    private val stopSignal: Signal,
    private val coordinator: Coordinator,
) : Thread("sender") {

    /** 是否处于丢弃模式 —— 丢到下一个 IDR 为止。 */
    private var dropping: Boolean = false

    override fun run() {
        try {
            while (true) {
                // 控制消息优先：心跳与控制应答饿不死。
                val control = controlQueue.poll()
                if (control != null) {
                    if (!writeControl(control)) return
                    continue
                }

                val frame = frameQueue.poll(SENDER_POLL_MS, TimeUnit.MILLISECONDS)
                if (frame != null) {
                    if (!sendFrame(frame)) return
                    continue
                }

                if (stopSignal.isRaised) break
            }
            drain()
        } catch (t: Throwable) {
            coordinator.reportFatal(
                Fail(FailLevel.FATAL, FailCode.UNEXPECTED, "路 #$routeId 的 sender 异常退出", t),
            )
        } finally {
            // 写侧由本线程自己拆 —— 只有它知道队列已经排空。
            // 幂等：收尾序列里还会再拆一次，用来唤醒卡在 write 里的情况。
            try {
                conn.shutdownOutput()
            } catch (e: Exception) {
                ln.d("路 #$routeId 拆写侧时抛异常：${e.message}")
            }
        }
    }

    /**
     * 发一帧（或按丢弃策略丢掉它）。
     *
     * @param frame 队列里的一帧
     * @return false 表示连接已坏，本线程应当退出
     */
    private fun sendFrame(frame: PendingFrame): Boolean {
        val waitedMs = (System.nanoTime() - frame.enqueueNanos) / 1_000_000

        if (dropping && !frame.keyFrame) {
            // 丢弃模式下只放行 IDR：其余帧丢掉，直到参考链重新接上。
            encoder.releaseOutputBuffer(frame.index)
            return true
        }

        if (!dropping && waitedMs > T_DROP_MS) {
            // 太旧了 ⇒ 丢 + 进入丢弃模式 + 请求一个新 IDR。
            dropping = true
            requestIdr()
            ln.d("路 #$routeId 帧已等待 ${waitedMs}ms > ${T_DROP_MS}ms ⇒ 丢弃并请求 IDR")
            encoder.releaseOutputBuffer(frame.index)
            return true
        }

        val ok = writeFrame(frame)
        // 无论成败都要把缓冲还回去，否则输出池会耗尽。
        encoder.releaseOutputBuffer(frame.index)
        if (ok && frame.keyFrame) dropping = false
        return ok
    }

    /**
     * 写一帧编码数据。
     *
     * @param frame 帧
     * @return false 表示写失败（连接已坏）
     */
    private fun writeFrame(frame: PendingFrame): Boolean {
        val prefix = ByteArray(FramePayload.HEADER_SIZE)
        prefix[0] = (if (frame.keyFrame) FramePayload.FLAG_KEY_FRAME else 0).toByte()
        Wire.writeLeLong(prefix, 1, frame.ptsUs)

        return when (val r = Wire.writeFrameStreaming(conn.output, Wire.TYPE_FRAME, prefix, frame.data)) {
            is Outcome.Ok -> true
            is Outcome.Err -> {
                ln.d("路 #$routeId ${r.fail.message}")
                false
            }
        }
    }

    /**
     * 写一条控制消息。
     *
     * 写失败**不重试**：字节流上写了一半再重发会污染流，只能判连接不可用。
     *
     * @param frame 待发帧
     * @return false 表示连接已坏
     */
    private fun writeControl(frame: Wire.Frame): Boolean =
        when (val r = Wire.writeFrame(conn.output, frame)) {
            is Outcome.Ok -> true
            is Outcome.Err -> {
                ln.d("路 #$routeId ${r.fail.message}")
                false
            }
        }

    /**
     * 把队列里剩下的消息发完。
     *
     * 收尾时生产者（`reader` / `link`）已经停了，所以非阻塞取空即结束；总时长另有上限兜底。
     */
    private fun drain() {
        val deadline = System.currentTimeMillis() + DRAIN_TIMEOUT_MS
        while (System.currentTimeMillis() < deadline) {
            val control = controlQueue.poll()
            if (control != null) {
                if (!writeControl(control)) return
                continue
            }
            val frame = frameQueue.poll() ?: break
            if (!writeFrame(frame)) {
                encoder.releaseOutputBuffer(frame.index)
                return
            }
            encoder.releaseOutputBuffer(frame.index)
        }
    }

    /** 常量。 */
    companion object {

        /** 队列空时的等待步长 —— 决定收尾信号的最坏响应延迟。 */
        private const val SENDER_POLL_MS: Long = 100

        /**
         * 排队延迟预算。
         *
         * 60fps × 200ms = 12 帧 < 队列容量 16 ⇒ 正常情况下本线程会先丢，入队失败很少发生。
         */
        private const val T_DROP_MS: Long = 200

        /** 排空队列的时间上限；超时放弃剩余，不阻塞收尾。 */
        private const val DRAIN_TIMEOUT_MS: Long = 1_000
    }
}
