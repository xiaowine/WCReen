package com.xiaowine.wcreen.agent.proto

import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome
import java.io.EOFException
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.SocketTimeoutException
import java.nio.ByteBuffer
import java.nio.channels.Channels

/**
 * 线协议的帧框架：`[1B 类型][4B 负载长度 N][N 字节负载]`。
 *
 * 长度是**负载长度**，不含头；上限 [MAX_PAYLOAD] 用来挡住对端声明一个巨大长度炸内存。
 * 字节序**小端** —— 两侧都是小端机器，但按「不依赖默认值」显式写死。
 *
 * 本对象只依赖 JDK 类型、不引用任何 Android 类型，所以协议编解码能进 JVM 单测
 * （见 `docs/spec/agent/code.md` §8）。
 */
internal object Wire {

    /** 帧头字节数：1 字节类型 + 4 字节负载长度。 */
    const val HEADER_SIZE: Int = 5

    /** 负载长度上限 16 MiB —— 超限即断连。协议不做分片，这是唯一的长度约束。 */
    const val MAX_PAYLOAD: Int = 16 * 1024 * 1024

    /** agent→PC 视频帧，负载是二进制（`flags + pts + Annex-B`）。 */
    const val TYPE_FRAME: Int = 0x01

    /** PC→agent 握手：协议版本。 */
    const val TYPE_HELLO: Int = 0x10

    /** agent→PC 握手应答：协议版本 + 自身状态。 */
    const val TYPE_HELLO_ACK: Int = 0x11

    /** PC→agent 请求能力表。 */
    const val TYPE_CAPS_REQUEST: Int = 0x12

    /** agent→PC 能力表。 */
    const val TYPE_CAPS: Int = 0x13

    /** PC→agent 指令，具体是哪条看 JSON 的 `cmd`。 */
    const val TYPE_COMMAND: Int = 0x20

    /** agent→PC 指令回报。 */
    const val TYPE_COMMAND_ACK: Int = 0x21

    /** agent→PC 主动上报。 */
    const val TYPE_EVENT: Int = 0x22

    /** 双向心跳，负载是空 JSON 对象。 */
    const val TYPE_HEARTBEAT: Int = 0x30

    /** 一帧：类型 + 负载。负载结构完全由类型决定。 */
    class Frame(val type: Int, val payload: ByteArray)

    /**
     * 读一帧（头 + 负载）。
     *
     * 先读定长头、校验长度上限、再读负载 ⇒ 读循环不分叉。
     *
     * @param input 连接读侧；调用方需保证此时确实有数据可读（本函数会阻塞到读满为止）
     * @return 成功给出帧；对端关闭、IO 失败、长度越界都返回对应级别的 [Outcome.Err]
     */
    fun readFrame(input: InputStream): Outcome<Frame> {
        val header = ByteArray(HEADER_SIZE)
        try {
            readFully(input, header, HEADER_SIZE)
        } catch (e: SocketTimeoutException) {
            return Outcome.Err(deadPeer(e))
        } catch (e: EOFException) {
            return Outcome.Err(Fail(FailLevel.STREAM, FailCode.PEER_CLOSED, "对端在帧头处关闭连接", e))
        } catch (e: IOException) {
            return Outcome.Err(Fail(FailLevel.STREAM, FailCode.IO, "读帧头失败", e))
        }

        val type = header[0].toInt() and 0xFF
        val length = readLeInt(header, 1)
        if (length < 0 || length > MAX_PAYLOAD) {
            return Outcome.Err(
                Fail(FailLevel.STREAM, FailCode.BAD_FRAME, "负载长度 $length 超出 0..$MAX_PAYLOAD"),
            )
        }

        val payload = ByteArray(length)
        try {
            readFully(input, payload, length)
        } catch (e: SocketTimeoutException) {
            return Outcome.Err(deadPeer(e))
        } catch (e: EOFException) {
            return Outcome.Err(Fail(FailLevel.STREAM, FailCode.PEER_CLOSED, "对端在负载中途关闭连接", e))
        } catch (e: IOException) {
            return Outcome.Err(Fail(FailLevel.STREAM, FailCode.IO, "读负载失败", e))
        }
        return Outcome.Ok(Frame(type, payload))
    }

    /**
     * 写一帧。
     *
     * 头单独写、数据再写 —— 合并成一次 `write` 反而要把整帧拷进一个缓冲区，
     * 而帧数据（S2 起是码流）应当零拷贝直写。
     *
     * @param output 连接写侧
     * @param frame 待写的帧
     * @return 成功，或 IO 失败（级别为单路 —— 连接坏了，其余路不受影响）
     */
    fun writeFrame(output: OutputStream, frame: Frame): Outcome<Unit> {
        val header = ByteArray(HEADER_SIZE)
        header[0] = frame.type.toByte()
        writeLeInt(header, 1, frame.payload.size)
        return try {
            output.write(header)
            output.write(frame.payload)
            output.flush()
            Outcome.Ok(Unit)
        } catch (e: IOException) {
            Outcome.Err(Fail(FailLevel.STREAM, FailCode.IO, "写帧失败（类型 ${frame.type}）", e))
        }
    }

    /**
     * 判活超时的统一失败对象。
     *
     * 只有不提供 fd 的传输（TCP）会走到这里 —— 有 fd 的传输走 `Os.poll`，
     * 超时在读之前就被判掉了。
     *
     * @param cause 读抛出的超时异常
     * @return 单路级失败：只断这一条连接
     */
    private fun deadPeer(cause: SocketTimeoutException): Fail = Fail(
        FailLevel.STREAM,
        FailCode.TIMEOUT,
        "读超时：判活窗口内没有收到任何消息 ⇒ 判对端掉线",
        cause,
    )

    /**
     * 写一帧，数据部分直接从 [ByteBuffer] 写出。
     *
     * 这是帧通路的写法：编码器的输出缓冲**不复制进 `ByteArray`**，直接把它的字节写进 socket
     * （`Channels.newChannel` 会把 `ByteBuffer` 的当前位置到 limit 全写出）。
     *
     * @param output 连接写侧
     * @param type 帧类型
     * @param prefix 负载里数据之前的部分（帧的 `flags + pts`）
     * @param body 负载里的数据部分；**只读**，本函数不改它的 position
     * @return 成功，或 IO 失败（单路级）
     */
    fun writeFrameStreaming(
        output: OutputStream,
        type: Int,
        prefix: ByteArray,
        body: ByteBuffer,
    ): Outcome<Unit> {
        val length = prefix.size + body.remaining()
        val header = ByteArray(HEADER_SIZE)
        header[0] = type.toByte()
        writeLeInt(header, 1, length)
        return try {
            output.write(header)
            output.write(prefix)
            val channel = Channels.newChannel(output)
            val view = body.duplicate()
            while (view.hasRemaining()) {
                channel.write(view)
            }
            output.flush()
            Outcome.Ok(Unit)
        } catch (e: IOException) {
            Outcome.Err(Fail(FailLevel.STREAM, FailCode.IO, "写帧失败（类型 $type）", e))
        }
    }

    /**
     * 读满 [length] 字节。
     *
     * @param input 数据源
     * @param buffer 目标缓冲，其长度必须 ≥ [length]
     * @param length 要读的字节数；为 0 时直接返回（`InputStream.read` 对 0 长度返回 0，
     *   若循环条件写成「读到的总数 < length」会空转）
     * @throws EOFException 对端提前关闭
     * @throws IOException 读失败
     */
    private fun readFully(input: InputStream, buffer: ByteArray, length: Int) {
        var read = 0
        while (read < length) {
            val n = input.read(buffer, read, length - read)
            if (n < 0) throw EOFException("期望 $length 字节，只读到 $read")
            read += n
        }
    }

    /**
     * 读小端 8 字节整数。
     *
     * @param buffer 数据源
     * @param offset 起始下标
     * @return 该处的 64 位整数
     */
    internal fun readLeLong(buffer: ByteArray, offset: Int): Long {
        var value = 0L
        for (i in 7 downTo 0) {
            value = (value shl 8) or (buffer[offset + i].toLong() and 0xFF)
        }
        return value
    }

    /**
     * 写小端 8 字节整数。
     *
     * @param buffer 目标缓冲
     * @param offset 起始下标
     * @param value 待写入的值
     */
    internal fun writeLeLong(buffer: ByteArray, offset: Int, value: Long) {
        for (i in 0 until 8) {
            buffer[offset + i] = (value ushr (8 * i)).toByte()
        }
    }

    /**
     * 读小端 4 字节整数。
     *
     * @param buffer 数据源
     * @param offset 起始下标
     * @return 该处的 32 位整数
     */
    internal fun readLeInt(buffer: ByteArray, offset: Int): Int =
        (buffer[offset].toInt() and 0xFF) or
            ((buffer[offset + 1].toInt() and 0xFF) shl 8) or
            ((buffer[offset + 2].toInt() and 0xFF) shl 16) or
            ((buffer[offset + 3].toInt() and 0xFF) shl 24)

    /**
     * 写小端 4 字节整数。
     *
     * @param buffer 目标缓冲
     * @param offset 起始下标
     * @param value 待写入的值
     */
    internal fun writeLeInt(buffer: ByteArray, offset: Int, value: Int) {
        buffer[offset] = value.toByte()
        buffer[offset + 1] = (value ushr 8).toByte()
        buffer[offset + 2] = (value ushr 16).toByte()
        buffer[offset + 3] = (value ushr 24).toByte()
    }
}
