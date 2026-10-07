package com.xiaowine.wcreen.agent.encode

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.view.Surface
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Ln
import com.xiaowine.wcreen.agent.runtime.Outcome
import java.nio.ByteBuffer

/**
 * 硬件编码器 —— 零拷贝通路的那一半。
 *
 * **owner 纪律**：`open` / `release` / `requestSyncFrame` 只允许在 `link` 线程调用；
 * `releaseOutputBuffer` 由 `sender` 调用 —— `MediaCodec` 是**全模型唯一被两条线程共享的对象**，
 * 不加锁（已实测跨线程 release 安全）。
 *
 * 三条硬约束：
 * - **绝不退软编** —— `open` 会回读 `isHardwareAccelerated`，不是硬编立即失败；
 * - `KEY_BIT_RATE` **必填**（不设 `configure` 直接抛）；
 * - 所有参数**显式设置**，不依赖设备默认（实测设备默认是 BT.601，且 `MediaFormat` 报的色彩字段与真值不符）。
 *
 * @param ln 日志出口
 */
internal class VideoEncoder(private val ln: Ln) {

    /** 底层编码器；null 表示未启动。 */
    private var codec: MediaCodec? = null

    /** 编码器的输入面 —— **屏的输出面就是它**（零拷贝）。 */
    private var inputSurface: Surface? = null

    /** 输入面；未启动时为 null。 */
    val surface: Surface? get() = inputSurface

    /**
     * 编码器单独给出的参数集（SPS/PPS）；**补到首个关键帧上**后清空（见 [poll]）。
     *
     * 只在"编码器不往 IDR 前带参数集"时才会被填上（实测：Android 15 模拟器的软编）。
     */
    private var csd: ByteArray? = null

    /** 是否已启动。 */
    val isRunning: Boolean get() = codec != null

    /**
     * 新建并启动编码器。
     *
     * @param params 已解析、已校验的参数
     * @return 编码器的输入面（供 `Screen.setSurface` 使用），或失败
     */
    fun open(params: EncoderParams): Outcome<Surface> {
        if (codec != null) return Outcome.Err(
            Fail(FailLevel.RECOVERABLE, FailCode.ENCODER_OPEN, "编码器已启动，参数变更必须先 release"),
        )

        val format = buildFormat(params)
        val created = try {
            MediaCodec.createByCodecName(params.encoderName)
        } catch (e: Exception) {
            return Outcome.Err(Fail(FailLevel.RECOVERABLE, FailCode.ENCODER_OPEN, "创建编码器失败：${e.message}", e))
        }

        return try {
            configure(created, params)
            val surface = created.createInputSurface()
            created.start()

            // 提示键（低延迟 / IDR 带 SPS-PPS / 无 B 帧 / realtime）走运行期参数。
            applyHints(created)

            // 「硬编属性」的一致性检查：能力表（`hardware`）是我们**上报给 PC 的承诺**，
            // 实际拿到的编码器必须与之一致 —— 不一致就是异常，记一条警告（**不再拒绝**：
            // 没有硬编时本来就允许软编回退，用户取向 2026-10-07）。
            val actualHardware = created.codecInfo.isHardwareAccelerated
            if (actualHardware != params.hardware) {
                ln.w(
                    "编码器 ${params.encoderName} 的硬编属性与能力表不一致：" +
                        "表=${params.hardware} 实=$actualHardware",
                )
            }

            codec = created
            inputSurface = surface
            ln.i(
                "编码器已启动：${params.encoderName} ${params.width}x${params.height}@${params.refreshRate} " +
                    "bitrate=${params.bitrate} mode=${params.bitrateModeName} profile=${params.profileName}",
            )
            Outcome.Ok(surface)
        } catch (e: Exception) {
            try {
                created.release()
            } catch (ignored: Exception) {
                ln.d("清理半启动的编码器失败：${ignored.message}")
            }
            // `MediaCodec.CodecException` 的 `message` 常为空 ⇒ 必须把**类名与诊断信息**带上，
            // 否则只看到一句"启动编码器失败"（实测踩过）。
            val detail = buildString {
                append(e.javaClass.simpleName)
                e.message?.takeIf { it.isNotEmpty() }?.let { append(": ").append(it) }
                if (e is MediaCodec.CodecException) {
                    append("（errorCode=").append(e.errorCode)
                    append("，recoverable=").append(e.isRecoverable)
                    append("，transient=").append(e.isTransient)
                    e.diagnosticInfo?.takeIf { it.isNotEmpty() }?.let { append("，诊断=").append(it) }
                    append("）")
                }
                // 带上当时用的 format —— `configure()` 抛的 `IllegalArgumentException` 常常**没有消息**，
                // 只有键值对能说明是哪个键不被接受（实测踩过）。
                append("；format=").append(format)
            }
            Outcome.Err(Fail(FailLevel.RECOVERABLE, FailCode.ENCODER_OPEN, "启动编码器失败：$detail", e))
        }
    }

    /**
     * 取一帧编码输出。
     *
     * ⚠️ **拿到 index 后不 release** —— 帧数据要等 `sender` 写完才放回去（零拷贝 + 背压）。
     *
     * @param timeoutUs 出队超时（微秒）；短超时保证指令能及时被处理
     * @return 一帧 / 暂时没有 / 编码器报错
     */
    fun poll(timeoutUs: Long): Poll {
        val c = codec ?: return Poll.Idle
        val info = MediaCodec.BufferInfo()
        val index = try {
            c.dequeueOutputBuffer(info, timeoutUs)
        } catch (e: MediaCodec.CodecException) {
            return Poll.Failed(
                Fail(FailLevel.STREAM, FailCode.ENCODER_FAILED, "编码器出错：${e.message}", e),
            )
        }

        if (index == MediaCodec.INFO_TRY_AGAIN_LATER) return Poll.Idle
        if (index < 0) return Poll.Idle

        if (info.size == 0) {
            // 空帧（例如格式变更标记）—— 立刻放回去，否则输出池会泄漏。
            releaseOutputBuffer(index)
            return Poll.Idle
        }

        if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
            // 有些编码器把参数集（SPS/PPS）**单独发一包**，它不能当视频帧发 —— 但也**不能丢**：
            // 若编码器不往 IDR 前带参数集（实测：Android 15 模拟器软编 `c2.android.avc.encoder`
            // 拒绝 `prepend-sps-pps-to-idr-frames` 提示键），首个 IDR 必须由我们补上，
            // 否则 PC 侧解码器报 `AVERROR_INVALIDDATA`。补的时机见下面 keyFrame 分支。
            csd = copyOf(c, index, info)
            releaseOutputBuffer(index)
            return Poll.Idle
        }

        val buffer = try {
            c.getOutputBuffer(index)
        } catch (e: Exception) {
            releaseOutputBuffer(index)
            return Poll.Failed(Fail(FailLevel.STREAM, FailCode.ENCODER_FAILED, "取输出缓冲失败", e))
        }
        if (buffer == null) {
            releaseOutputBuffer(index)
            return Poll.Failed(Fail(FailLevel.STREAM, FailCode.ENCODER_FAILED, "输出缓冲为 null"))
        }

        // 只暴露这一帧的范围；调用方不再改 position/limit，所以跨线程只读是安全的。
        buffer.position(info.offset)
        buffer.limit(info.offset + info.size)
        val slice = buffer.slice()

        val keyFrame = (info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME) != 0
        val prefix = csd
        if (keyFrame && prefix != null) {
            // 首个关键帧：把参数集补在前面（`annex-B` 里 SPS/PPS 与 IDR 直接拼接即可解码）。
            // 这是**零拷贝纪律的唯一例外**：只在这一帧上做一次拷贝。
            csd = null
            val merged = ByteBuffer.allocate(prefix.size + slice.remaining())
            merged.put(prefix)
            merged.put(slice)
            merged.flip()
            return Poll.Frame(index, keyFrame, info.presentationTimeUs, merged)
        }
        return Poll.Frame(index, keyFrame, info.presentationTimeUs, slice)
    }

    /**
     * 拷出一段输出缓冲的字节（仅用于参数集 —— 它很小）。
     *
     * @param c 编码器
     * @param index 输出缓冲下标
     * @param info 缓冲信息（给出 offset / size）
     * @return 参数集字节；取不到时 null
     */
    private fun copyOf(c: MediaCodec, index: Int, info: MediaCodec.BufferInfo): ByteArray? = try {
        val buf = c.getOutputBuffer(index)
        if (buf == null) {
            null
        } else {
            buf.position(info.offset)
            buf.limit(info.offset + info.size)
            ByteArray(info.size).also { buf.get(it) }
        }
    } catch (e: Exception) {
        ln.w("取编码器参数集失败：${e.message}")
        null
    }

    /**
     * 把输出缓冲还给编码器。
     *
     * @param index `poll` 给出的缓冲下标
     */
    fun releaseOutputBuffer(index: Int) {
        val c = codec ?: return
        try {
            c.releaseOutputBuffer(index, false)
        } catch (e: Exception) {
            ln.w("释放输出缓冲 $index 失败：${e.message}")
        }
    }

    /**
     * 请求一个同步帧（IDR）。
     *
     * ⚠️ 编码器**必须有输入帧**才能产 IDR —— 静止屏上单独请求无效，
     * 所以调用方要按「换面 + 请求」的组合来用（`encode-lifecycle.md` §1.3）。
     */
    fun requestSyncFrame() {
        val c = codec ?: return
        try {
            c.setParameters(android.os.Bundle().apply { putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0) })
        } catch (e: Exception) {
            ln.w("请求同步帧失败：${e.message}")
        }
    }

    /**
     * 释放编码器。
     *
     * ⚠️ **必须在 `Screen.release()` 之后调用**（硬顺序）：反过来的话，屏还在往一个即将失效的
     * 面上写。幂等。
     */
    fun release() {
        val c = codec ?: return
        codec = null
        inputSurface = null
        csd = null
        try {
            c.stop()
        } catch (e: Exception) {
            ln.d("停编码器时抛异常：${e.message}")
        }
        try {
            c.release()
            ln.i("编码器已释放")
        } catch (e: Exception) {
            ln.w("释放编码器时抛异常：${e.message}")
        }
    }

    /**
     * 配置编码器 —— **提示键不进 `format`**，一律在 `start()` 之后走 [MediaCodec.setParameters]。
     *
     * 实测（Android 15 模拟器 / `c2.android.avc.encoder`）：`configure()` 对
     * `prepend-sps-pps-to-idr-frames` 抛 `IllegalArgumentException` 且**消息为空**；
     * 硬编（真机 `c2.qti.avc.encoder`）照常接受。提示键放在运行期参数里两边都能用。
     *
     * @param c 已创建的编码器
     * @param p 已解析的参数
     */
    private fun configure(c: MediaCodec, p: EncoderParams) {
        c.configure(buildFormat(p), null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
    }

    /**
     * 下发**运行期提示键**（低延迟 / IDR 带 SPS-PPS / 无 B 帧 / realtime 优先级）。
     *
     * 为什么走这里而不是 `format`：某些编码器不接受这些键出现在 `format` 里（见 [configure]），
     * 而它们本身都只是**提示** ⇒ 拿不到也不影响正确性，故失败只记一条日志。
     *
     * @param c 已启动的编码器
     */
    private fun applyHints(c: MediaCodec) {
        try {
            val hints = android.os.Bundle().apply {
                putInt("low-latency", 1)
                // 每个 IDR 自带 SPS/PPS ⇒ 新客户端拿到任意 IDR 即可解码，协议里不需要 CSD 字段。
                putInt(KEY_PREPEND_SPS_PPS, 1)
                putInt(KEY_MAX_B_FRAMES, 0)
                putInt(MediaFormat.KEY_PRIORITY, 0)
            }
            c.setParameters(hints)
        } catch (e: Exception) {
            ln.d("下发运行期提示键失败（不影响正确性）：${e.message}")
        }
    }

    /**
     * 组装 `MediaFormat` —— 每一项都显式写死，**不依赖设备默认**
     * （`docs/spec/agent/encode-params.md` §6 的显式设置总表）。
     *
     * 只放**必须**项；提示项走 [applyHints]。
     *
     * @param p 已解析的参数
     * @return 可直接 `configure` 的格式
     */
    private fun buildFormat(p: EncoderParams): MediaFormat {
        val format = MediaFormat.createVideoFormat(p.codec.mime, p.width, p.height)
        // 零拷贝前提。
        format.setInteger(
            MediaFormat.KEY_COLOR_FORMAT,
            MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface,
        )
        // 必填：不设它 configure() 直接抛 IllegalArgumentException，无论是否设了码率模式。
        format.setInteger(MediaFormat.KEY_BIT_RATE, p.bitrate)
        format.setInteger(MediaFormat.KEY_BITRATE_MODE, p.bitrateMode)
        // 帧率 = 刷新率；它不控制实际出帧（由屏驱动），只影响码率控制的比特分配。
        format.setInteger(MediaFormat.KEY_FRAME_RATE, p.refreshRate)
        format.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, CodecCaps.I_FRAME_INTERVAL_SECONDS)
        format.setInteger(MediaFormat.KEY_PROFILE, p.profileId)
        // 色彩三项一律显式设置：设备默认是 BT.601，且 MediaFormat 报的值与真值不符。
        format.setInteger(MediaFormat.KEY_COLOR_STANDARD, p.colorStandard)
        format.setInteger(MediaFormat.KEY_COLOR_RANGE, p.colorRange)
        format.setInteger(MediaFormat.KEY_COLOR_TRANSFER, p.colorTransfer)
        return format
    }

    /**
     * 一次出队的结果。
     */
    sealed class Poll {

        /**
         * 拿到一帧。
         *
         * @property index 输出缓冲下标 —— 写完数据后必须 `releaseOutputBuffer(index)`
         * @property keyFrame 是否关键帧（IDR）
         * @property ptsUs 呈现时间戳（微秒）
         * @property data 编码数据（Annex-B），**只读**，跨线程安全
         */
        class Frame(
            val index: Int,
            val keyFrame: Boolean,
            val ptsUs: Long,
            val data: ByteBuffer,
        ) : Poll()

        /** 暂时没有帧（短超时到期或格式变更标记）。 */
        object Idle : Poll()

        /**
         * 编码器报错。
         *
         * @property fail 单路级失败 —— 由 `link` 决定拆这一路
         */
        class Failed(val fail: Fail) : Poll()
    }

    /** 没有公开常量的两个键 —— 名字只在这一处出现。 */
    private companion object {

        /** 每个同步帧前插 SPS/PPS。 */
        const val KEY_PREPEND_SPS_PPS = "prepend-sps-pps-to-idr-frames"

        /** 最大 B 帧数。 */
        const val KEY_MAX_B_FRAMES = "max-bframes"
    }
}
