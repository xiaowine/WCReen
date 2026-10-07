package com.xiaowine.wcreen.agent.proto

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.google.gson.JsonSyntaxException
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome
import java.nio.charset.StandardCharsets
import java.util.zip.CRC32

/**
 * 控制消息负载的编解码：`[4B CRC32][UTF-8 JSON]`。
 *
 * **CRC 是「失步检测器 + 实现自检」，不是防线路噪声** —— 传输层（adb / TCP）自带校验，
 * 真正的问题是「读循环一旦读错长度，后面全部乱套且无法自我恢复」。
 * 所以 CRC 覆盖 `[类型][长度][JSON]`，**连头部一起保护**：头部损坏最危险。
 *
 * 帧（`0x01`）不带 CRC：它的长度字段损坏时，下一条控制消息的 CRC 会失败而断连重连，
 * 心跳每 3s 一条 ⇒ 最迟一个心跳周期内发现。
 *
 * 只依赖 JDK + Gson，不引用任何 Android 类型 ⇒ 可进 JVM 单测。
 */
internal object Codec {

    /** 负载里 CRC 字段占的字节数。 */
    const val CRC_SIZE: Int = 4

    /** Gson 实例 —— 无自定义 TypeAdapter，DTO 形态由 `Control.kt` 的纪律保证。 */
    private val gson: Gson = Gson()

    /**
     * 把控制消息编成负载。
     *
     * @param type 帧类型，参与 CRC 计算
     * @param msg 任意 DTO 实例
     * @return `[4B CRC32][UTF-8 JSON]`；CRC 与长度均为**小端**
     */
    fun encode(type: Int, msg: Any): ByteArray {
        val json = gson.toJson(msg).toByteArray(StandardCharsets.UTF_8)
        val payloadLength = CRC_SIZE + json.size

        val crc = CRC32()
        crc.update(type)
        crc.update(leIntBytes(payloadLength))
        crc.update(json)

        val payload = ByteArray(payloadLength)
        Wire.writeLeInt(payload, 0, crc.value.toInt())
        json.copyInto(payload, CRC_SIZE)
        return payload
    }

    /**
     * 校验 CRC 并反序列化。
     *
     * @param type 帧类型，参与 CRC 计算
     * @param payload 收到的完整负载（含首 4 字节 CRC）
     * @param clazz 目标 DTO 类型
     * @return 成功给出 DTO；长度不足 / CRC 不匹配 / JSON 非法 / 解析成 null 都返回对应级别的失败
     */
    fun <T : Any> decode(type: Int, payload: ByteArray, clazz: Class<T>): Outcome<T> {
        if (payload.size < CRC_SIZE) {
            return Outcome.Err(
                Fail(FailLevel.STREAM, FailCode.BAD_CRC, "控制消息负载只有 ${payload.size} 字节，不足 $CRC_SIZE"),
            )
        }

        val expected = Wire.readLeInt(payload, 0)
        val crc = CRC32()
        crc.update(type)
        crc.update(leIntBytes(payload.size))
        crc.update(payload, CRC_SIZE, payload.size - CRC_SIZE)
        val actual = crc.value.toInt()
        if (actual != expected) {
            return Outcome.Err(
                Fail(
                    FailLevel.STREAM,
                    FailCode.BAD_CRC,
                    "CRC32 不匹配（收到 $expected，算出 $actual）—— 读循环已失步，只能断连重连",
                ),
            )
        }

        val json = String(payload, CRC_SIZE, payload.size - CRC_SIZE, StandardCharsets.UTF_8)
        val msg = try {
            gson.fromJson(json, clazz)
        } catch (e: JsonSyntaxException) {
            return Outcome.Err(Fail(FailLevel.STREAM, FailCode.BAD_JSON, "JSON 解析失败：$json", e))
        }
        // "null" 也是合法 JSON，Gson 会给出 null —— 这属于协议错误，不能当成"空消息"放过。
        return if (msg == null) {
            Outcome.Err(Fail(FailLevel.STREAM, FailCode.BAD_JSON, "JSON 是 null：$json"))
        } else {
            Outcome.Ok(msg)
        }
    }

    /**
     * 把信封里的 `payload` 解成载荷结构体 —— **第二段解析**。
     *
     * 载荷已经是解过一次的 [JsonObject]（不是带 CRC 的字节）⇒ 这里只按 `cmd` 决定的目标类型解，
     * 不再重复解析整包。
     *
     * @param payload 信封里取出的载荷对象；null 表示该指令**不带载荷**
     * @param clazz 载荷结构体类型（`ApplyScreenArgs` / `InputArgs` …）
     * @param empty 空载荷时给出的默认结构体
     * @return 成功给出载荷结构体；JSON 形状不合法返回协议级失败
     */
    fun <T : Any> decodePayload(payload: JsonObject?, clazz: Class<T>, empty: () -> T): Outcome<T> {
        if (payload == null) return Outcome.Ok(empty())
        val args = try {
            gson.fromJson(payload, clazz)
        } catch (e: JsonSyntaxException) {
            return Outcome.Err(Fail(FailLevel.STREAM, FailCode.BAD_JSON, "载荷解析失败：$payload", e))
        }
        // "null" 也是合法 JSON，Gson 会给出 null —— 这属于协议错误，不能当成"空载荷"放过。
        return if (args == null) {
            Outcome.Err(Fail(FailLevel.STREAM, FailCode.BAD_JSON, "载荷是 null：$payload"))
        } else {
            Outcome.Ok(args)
        }
    }

    /**
     * 把小端整数编成 4 字节，供 [CRC32.update] 逐字节喂入。
     *
     * @param value 待编码的值
     * @return 长度为 4 的字节数组
     */
    private fun leIntBytes(value: Int): ByteArray = ByteArray(4).also { Wire.writeLeInt(it, 0, value) }
}
