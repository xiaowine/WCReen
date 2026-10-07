package com.xiaowine.wcreen.agent.proto

import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome

/**
 * 视频帧（`0x01`）的负载编解码：`[1B flags][8B 小端 pts_us][编码数据]`。
 *
 * 两条设计要点：
 * - `flags` **显式给**（bit0 = 关键帧），不让 PC 去扫 NAL —— 全局规则「不依赖默认值」；
 * - `pts_us` 是 `MediaCodec` 的 `presentationTimeUs`，PC 做渲染节拍 / 抖动缓冲要用。
 *
 * 帧**不带 CRC**：长度字段损坏会让读循环失步，但下一条控制消息的 CRC 会失败 ⇒ 断连重连；
 * 心跳每 3s 一条 ⇒ 最迟一个心跳周期内发现（`transport-protocol.md` §3.3）。
 *
 * 只依赖 JDK 类型 ⇒ 可进 JVM 单测。
 */
internal object FramePayload {

    /** 负载头字节数：1 字节 flags + 8 字节 pts。 */
    const val HEADER_SIZE: Int = 9

    /** `flags` bit0：本帧是关键帧（IDR）。 */
    const val FLAG_KEY_FRAME: Int = 1

    /**
     * 编码一帧的负载。
     *
     * @param keyFrame 是否关键帧
     * @param ptsUs 呈现时间戳（微秒）
     * @param data 编码数据（Annex-B）
     * @return `[1B flags][8B 小端 pts][data]`
     */
    fun encode(keyFrame: Boolean, ptsUs: Long, data: ByteArray): ByteArray {
        val payload = ByteArray(HEADER_SIZE + data.size)
        payload[0] = (if (keyFrame) FLAG_KEY_FRAME else 0).toByte()
        Wire.writeLeLong(payload, 1, ptsUs)
        data.copyInto(payload, HEADER_SIZE)
        return payload
    }

    /**
     * 解出一帧。
     *
     * @param payload 收到的负载
     * @return 帧信息；负载不足 [HEADER_SIZE] 字节时报单路级失败
     */
    fun decode(payload: ByteArray): Outcome<Frame> {
        if (payload.size < HEADER_SIZE) {
            return Outcome.Err(
                Fail(
                    FailLevel.STREAM,
                    FailCode.BAD_FRAME,
                    "帧负载只有 ${payload.size} 字节，不足 $HEADER_SIZE",
                ),
            )
        }
        val keyFrame = (payload[0].toInt() and FLAG_KEY_FRAME) != 0
        val ptsUs = Wire.readLeLong(payload, 1)
        val data = payload.copyOfRange(HEADER_SIZE, payload.size)
        return Outcome.Ok(Frame(keyFrame, ptsUs, data))
    }

    /**
     * 一帧的内容。
     *
     * @property keyFrame 是否关键帧
     * @property ptsUs 呈现时间戳（微秒）
     * @property data 编码数据（Annex-B）
     */
    class Frame(val keyFrame: Boolean, val ptsUs: Long, val data: ByteArray)
}
