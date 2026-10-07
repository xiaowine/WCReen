package com.xiaowine.wcreen.agent.proto

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.failOrNull
import com.xiaowine.wcreen.agent.runtime.valueOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * [Codec] 的控制消息编解码测试。
 *
 * CRC 在这里的作用是**失步检测 + 实现自检**，所以重点测「头部被改动也要被发现」——
 * 头部损坏最危险，覆盖范围必须含类型与长度。
 */
class CodecTest {

    @Test
    fun `往返一致`() {
        val payload = Codec.encode(Wire.TYPE_HELLO, HelloMsg(7))
        val msg = Codec.decode(Wire.TYPE_HELLO, payload, HelloMsg::class.java).valueOrNull()
        assertNotNull(msg)
        assertEquals(7, msg!!.version)
    }

    @Test
    fun `CRC 覆盖类型`() {
        val payload = Codec.encode(Wire.TYPE_HELLO, HelloMsg(7))
        // 用另一个类型解码 ⇒ 类型没被覆盖时这里会"成功"，那就说明保护范围错了。
        val fail = Codec.decode(Wire.TYPE_COMMAND, payload, HelloMsg::class.java).failOrNull()
        assertEquals(FailCode.BAD_CRC, fail!!.code)
        assertEquals(FailLevel.STREAM, fail.level)
    }

    @Test
    fun `负载被改动即发现`() {
        val payload = Codec.encode(Wire.TYPE_COMMAND, CommandMsg(Cmd.STOP_AGENT, 3))
        payload[Codec.CRC_SIZE + 1] = (payload[Codec.CRC_SIZE + 1] + 1).toByte()
        assertEquals(FailCode.BAD_CRC, Codec.decode(Wire.TYPE_COMMAND, payload, CommandMsg::class.java).failOrNull()!!.code)
    }

    @Test
    fun `负载不足 CRC 长度`() {
        assertEquals(FailCode.BAD_CRC, Codec.decode(Wire.TYPE_HELLO, ByteArray(3), HelloMsg::class.java).failOrNull()!!.code)
    }

    @Test
    fun `JSON 非法`() {
        // 手搓一个 CRC 正确的负载，但 JSON 不是对象。
        val json = "{ 这不是 JSON".toByteArray(Charsets.UTF_8)
        val payload = ByteArray(Codec.CRC_SIZE + json.size)
        json.copyInto(payload, Codec.CRC_SIZE)
        val crc = java.util.zip.CRC32()
        crc.update(Wire.TYPE_HELLO)
        crc.update(ByteArray(4).also { Wire.writeLeInt(it, 0, payload.size) })
        crc.update(json)
        Wire.writeLeInt(payload, 0, crc.value.toInt())

        assertEquals(FailCode.BAD_JSON, Codec.decode(Wire.TYPE_HELLO, payload, HelloMsg::class.java).failOrNull()!!.code)
    }

    @Test
    fun `JSON 是 null 也算协议错误`() {
        val json = "null".toByteArray(Charsets.UTF_8)
        val payload = ByteArray(Codec.CRC_SIZE + json.size)
        json.copyInto(payload, Codec.CRC_SIZE)
        val crc = java.util.zip.CRC32()
        crc.update(Wire.TYPE_HELLO)
        crc.update(ByteArray(4).also { Wire.writeLeInt(it, 0, payload.size) })
        crc.update(json)
        Wire.writeLeInt(payload, 0, crc.value.toInt())

        assertEquals(FailCode.BAD_JSON, Codec.decode(Wire.TYPE_HELLO, payload, HelloMsg::class.java).failOrNull()!!.code)
    }

    @Test
    fun `心跳是空对象`() {
        val payload = Codec.encode(Wire.TYPE_HEARTBEAT, HeartbeatMsg())
        assertEquals("{}", String(payload, Codec.CRC_SIZE, payload.size - Codec.CRC_SIZE, Charsets.UTF_8))
        assertNotNull(Codec.decode(Wire.TYPE_HEARTBEAT, payload, HeartbeatMsg::class.java).valueOrNull())
    }

    @Test
    fun `错误对象的 class 字段走 SerializedName`() {
        // Kotlin 关键字不能当属性名，线上字段名必须是 "class"。
        val payload = Codec.encode(
            Wire.TYPE_COMMAND_ACK,
            CommandAckMsg(requestId = 1, ok = false, error = ErrorMsg("stream", "x", "y")),
        )
        val json = String(payload, Codec.CRC_SIZE, payload.size - Codec.CRC_SIZE, Charsets.UTF_8)
        assertEquals(true, json.contains("\"class\":\"stream\""))
        assertNull(Codec.decode(Wire.TYPE_COMMAND_ACK, payload, CommandAckMsg::class.java).failOrNull())
    }

    @Test
    fun `缺省值生效`() {
        // DTO 全字段带默认值 ⇒ Gson 走构造器路径 ⇒ 缺字段时不是 null。
        val payload = Codec.encode(Wire.TYPE_COMMAND, CommandMsg())
        val msg = Codec.decode(Wire.TYPE_COMMAND, payload, CommandMsg::class.java).valueOrNull()
        assertEquals("", msg!!.cmd)
        assertEquals(0, msg.requestId)
        assertNull(msg.payload)
    }

    /** 取出负载里的 JSON 文本（跳过首 4 字节 CRC）。 */
    private fun jsonOf(payload: ByteArray): String =
        String(payload, Codec.CRC_SIZE, payload.size - Codec.CRC_SIZE, Charsets.UTF_8)

    @Test
    fun `指令帧是信封加 payload 的形状`() {
        // 有字段指令：载荷落在 "payload" 里，指令自己的字段不再平铺在顶层。
        val body: JsonObject = Gson().fromJson("""{"streamId":0,"params":{}}""", JsonObject::class.java)
        val apply = jsonOf(Codec.encode(Wire.TYPE_COMMAND, CommandMsg(Cmd.APPLY_SCREEN, 7, body)))
        assertTrue(apply.contains("\"cmd\":\"applyScreen\""))
        assertTrue(apply.contains("\"requestId\":7"))
        assertTrue(apply.contains("\"payload\":{"))

        // 无字段指令：省略 payload。
        val stop = jsonOf(Codec.encode(Wire.TYPE_COMMAND, CommandMsg(Cmd.STOP_AGENT, 9)))
        assertTrue(stop.contains("\"cmd\":\"stopAgent\""))
        assertTrue(stop.contains("\"requestId\":9"))
        assertFalse(stop.contains("payload"))
    }

    @Test
    fun `payload 缺省时按空载荷解`() {
        // 无字段指令（stopAgent）不带 payload ⇒ decodePayload 给出默认结构体，不报错。
        val empty = Codec.decodePayload(null, ApplyScreenArgs::class.java) { ApplyScreenArgs() }.valueOrNull()
        assertNotNull(empty)
        assertEquals(0, empty!!.streamId)
        assertEquals(0, empty.params.width)
    }
}
