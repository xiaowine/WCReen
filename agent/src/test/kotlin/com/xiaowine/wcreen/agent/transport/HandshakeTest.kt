package com.xiaowine.wcreen.agent.transport

import com.xiaowine.wcreen.agent.proto.ErrorCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.failOrNull
import com.xiaowine.wcreen.agent.runtime.valueOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * [Handshake] 的版本校验测试。
 *
 * 版本必须精确相等 —— 不等就要立刻发现「设备上跑的是旧 jar」。
 */
class HandshakeTest {

    @Test
    fun `版本相等即通过`() {
        assertNotNull(Handshake.checkVersion(Handshake.PROTOCOL_VERSION).valueOrNull())
    }

    @Test
    fun `版本不等即失败`() {
        val older = Handshake.checkVersion(Handshake.PROTOCOL_VERSION - 1).failOrNull()
        assertEquals(ErrorCode.VERSION_MISMATCH, older!!.code)
        assertEquals(FailLevel.STREAM, older.level)

        val newer = Handshake.checkVersion(Handshake.PROTOCOL_VERSION + 1).failOrNull()
        assertEquals(ErrorCode.VERSION_MISMATCH, newer!!.code)
    }

    @Test
    fun `协议版本当前为 1`() {
        // 改这个值等于改协议契约，必须同时改 PC 侧 —— 用一条断言把它钉住。
        assertEquals(1, Handshake.PROTOCOL_VERSION)
        assertNull(Handshake.checkVersion(0).valueOrNull())
    }
}
