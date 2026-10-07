package com.xiaowine.wcreen.agent.proto

import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.failOrNull
import com.xiaowine.wcreen.agent.runtime.valueOrNull
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.InputStream

/**
 * [Wire] 的帧读写测试。
 *
 * 读循环是**不分叉**的（定长头 → 长度 → 负载），一旦长度解析错了整条流就失步，
 * 所以这里重点盯长度与字节序。
 */
class WireTest {

    @Test
    fun `往返一致`() {
        val payload = byteArrayOf(0x01, 0x02, 0x03)
        val out = ByteArrayOutputStream()
        Wire.writeFrame(out, Wire.Frame(Wire.TYPE_COMMAND, payload))

        val frame = Wire.readFrame(ByteArrayInputStream(out.toByteArray())).valueOrNull()
        assertNotNull(frame)
        assertEquals(Wire.TYPE_COMMAND, frame!!.type)
        assertArrayEquals(payload, frame.payload)
    }

    @Test
    fun `帧头是小端且长度只算负载`() {
        val out = ByteArrayOutputStream()
        Wire.writeFrame(out, Wire.Frame(0x21, ByteArray(258)))
        val bytes = out.toByteArray()

        assertEquals(0x21, bytes[0].toInt())
        // 258 = 0x0102 ⇒ 小端是 02 01 00 00
        assertEquals(0x02, bytes[1].toInt())
        assertEquals(0x01, bytes[2].toInt())
        assertEquals(0x00, bytes[3].toInt())
        assertEquals(0x00, bytes[4].toInt())
        assertEquals(Wire.HEADER_SIZE + 258, bytes.size)
    }

    @Test
    fun `空负载合法`() {
        val out = ByteArrayOutputStream()
        Wire.writeFrame(out, Wire.Frame(Wire.TYPE_HEARTBEAT, ByteArray(0)))
        val frame = Wire.readFrame(ByteArrayInputStream(out.toByteArray())).valueOrNull()
        assertEquals(0, frame!!.payload.size)
    }

    @Test
    fun `长度越界即拒绝`() {
        // 声明 16 MiB + 1 ⇒ 必须拒绝，不能真去分配。
        val header = ByteArray(Wire.HEADER_SIZE)
        header[0] = Wire.TYPE_COMMAND.toByte()
        Wire.writeLeInt(header, 1, Wire.MAX_PAYLOAD + 1)
        val fail = Wire.readFrame(ByteArrayInputStream(header)).failOrNull()
        assertEquals(FailCode.BAD_FRAME, fail!!.code)
    }

    @Test
    fun `对端在头处关闭`() {
        val fail = Wire.readFrame(ByteArrayInputStream(ByteArray(2))).failOrNull()
        assertEquals(FailCode.PEER_CLOSED, fail!!.code)
    }

    @Test
    fun `对端在负载中途关闭`() {
        val out = ByteArrayOutputStream()
        Wire.writeFrame(out, Wire.Frame(Wire.TYPE_COMMAND, ByteArray(10)))
        val truncated = out.toByteArray().copyOf(Wire.HEADER_SIZE + 3)
        val fail = Wire.readFrame(ByteArrayInputStream(truncated)).failOrNull()
        assertEquals(FailCode.PEER_CLOSED, fail!!.code)
    }

    @Test
    fun `一次只读到一部分也能读满`() {
        val out = ByteArrayOutputStream()
        Wire.writeFrame(out, Wire.Frame(Wire.TYPE_FRAME, ByteArray(100) { it.toByte() }))
        // 每次只给 7 字节的流，专门验证 readFully 的循环。
        val frame = Wire.readFrame(DripStream(out.toByteArray(), 7)).valueOrNull()
        assertEquals(100, frame!!.payload.size)
    }

    @Test
    fun `整数小端读写`() {
        val buffer = ByteArray(4)
        Wire.writeLeInt(buffer, 0, 0x01020304)
        assertArrayEquals(byteArrayOf(0x04, 0x03, 0x02, 0x01), buffer)
        assertEquals(0x01020304, Wire.readLeInt(buffer, 0))
        assertEquals(0, Wire.readLeInt(ByteArray(4), 0))
    }

    /** 每次最多返回 [chunk] 字节的输入流 —— 模拟 TCP 的分段到达。 */
    private class DripStream(private val data: ByteArray, private val chunk: Int) : InputStream() {

        private var pos = 0

        override fun read(): Int = if (pos < data.size) data[pos++].toInt() and 0xFF else -1

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            if (pos >= data.size) return -1
            val n = minOf(len, chunk, data.size - pos)
            System.arraycopy(data, pos, b, off, n)
            pos += n
            return n
        }
    }
}
