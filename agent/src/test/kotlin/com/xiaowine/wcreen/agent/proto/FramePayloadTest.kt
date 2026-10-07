package com.xiaowine.wcreen.agent.proto

import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.failOrNull
import com.xiaowine.wcreen.agent.runtime.valueOrNull
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * [FramePayload] 的帧负载编解码测试。
 *
 * 帧头是 PC 侧解码的起点，`flags` 与 `pts` 错一位就全错。
 */
class FramePayloadTest {

    @Test
    fun `往返一致`() {
        val data = byteArrayOf(0, 0, 1, 0x65, 0x11, 0x22)
        val payload = FramePayload.encode(keyFrame = true, ptsUs = 1234567L, data = data)
        val frame = FramePayload.decode(payload).valueOrNull()

        assertNotNull(frame)
        assertTrue(frame!!.keyFrame)
        assertEquals(1234567L, frame.ptsUs)
        assertArrayEquals(data, frame.data)
    }

    @Test
    fun `非关键帧的 flags 为 0`() {
        val payload = FramePayload.encode(keyFrame = false, ptsUs = 0L, data = ByteArray(3))
        assertEquals(0, payload[0].toInt())
        val frame = FramePayload.decode(payload).valueOrNull()
        assertFalse(frame!!.keyFrame)
    }

    @Test
    fun `pts 是小端 8 字节`() {
        val payload = FramePayload.encode(keyFrame = false, ptsUs = 0x0102030405060708L, data = ByteArray(0))
        assertEquals(FramePayload.HEADER_SIZE, payload.size)
        assertEquals(0x08, payload[1].toInt())
        assertEquals(0x01, payload[8].toInt())
        assertEquals(0x0102030405060708L, FramePayload.decode(payload).valueOrNull()!!.ptsUs)
    }

    @Test
    fun `空数据合法`() {
        val payload = FramePayload.encode(keyFrame = true, ptsUs = 1L, data = ByteArray(0))
        assertEquals(FramePayload.HEADER_SIZE, payload.size)
        assertEquals(0, FramePayload.decode(payload).valueOrNull()!!.data.size)
    }

    @Test
    fun `负载不足即拒绝`() {
        val fail = FramePayload.decode(ByteArray(FramePayload.HEADER_SIZE - 1)).failOrNull()
        assertEquals(FailCode.BAD_FRAME, fail!!.code)
    }

    @Test
    fun `能力表与申请屏幕 DTO 走 Gson 往返`() {
        val caps = CapsMsg(
            requestId = 5,
            codecs = listOf(
                CodecMsg(
                    name = "c2.qti.avc.encoder",
                    mime = "video/avc",
                    codec = "H.264",
                    hardware = true,
                    bitDepths = listOf(8),
                    profiles = listOf("Baseline", "High"),
                    colorStandards = listOf("BT709"),
                    colorRanges = listOf("FULL"),
                    colorTransfers = listOf("SDR_VIDEO"),
                    width = RangeMsg(128, 8192, 2),
                    height = RangeMsg(128, 8192, 2),
                    refreshRate = RangeMsg(1, 480, 1),
                    bitrate = RangeMsg(1, 220000000, 1),
                    maxInstances = 16,
                ),
            ),
            defaults = DefaultsMsg(codec = "H.264", bitrateBpp = 0.1, refreshRate = 60),
        )
        val payload = Codec.encode(Wire.TYPE_CAPS, caps)
        val decoded = Codec.decode(Wire.TYPE_CAPS, payload, CapsMsg::class.java).valueOrNull()

        assertNotNull(decoded)
        assertEquals(5, decoded!!.requestId)
        assertEquals(1, decoded.codecs.size)
        assertEquals("H.264", decoded.codecs[0].codec)
        assertEquals(2, decoded.codecs[0].width.step)
        assertEquals(16, decoded.codecs[0].maxInstances)
        assertEquals(0.1, decoded.defaults.bitrateBpp, 1e-9)
    }

    @Test
    fun `申请屏幕的参数全缺省时不崩`() {
        val apply = ApplyScreenArgs(streamId = 0)
        val payload = Codec.encode(Wire.TYPE_COMMAND, apply)
        val decoded = Codec.decode(Wire.TYPE_COMMAND, payload, ApplyScreenArgs::class.java).valueOrNull()

        assertNotNull(decoded)
        assertEquals(0, decoded!!.params.width)
        assertEquals("", decoded.params.codec)
        assertEquals(0.0, decoded.params.bitrateBpp, 1e-9)
    }

    @Test
    fun `回报里的 applied 带 streamId`() {
        val ack = CommandAckMsg(
            requestId = 7,
            ok = true,
            result = ResultMsg(3, AppliedMsg(streamId = 3, width = 1920, height = 1080)),
        )
        val payload = Codec.encode(Wire.TYPE_COMMAND_ACK, ack)
        val json = String(payload, Codec.CRC_SIZE, payload.size - Codec.CRC_SIZE, Charsets.UTF_8)
        assertTrue(json.contains("\"streamId\":3"))

        val decoded = Codec.decode(Wire.TYPE_COMMAND_ACK, payload, CommandAckMsg::class.java).valueOrNull()
        assertEquals(3, decoded!!.result!!.streamId)
        assertEquals(1920, decoded.result!!.applied!!.width)
        assertNull(decoded.error)
    }
}
