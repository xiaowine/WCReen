package com.xiaowine.wcreen.agent.runtime

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * [Options] 的解析测试。
 *
 * CLI 是脚本与工具链的接口（`runtime-startup.md` §1.1），改错了要能立刻发现。
 */
class OptionsTest {

    @Test
    fun `adb 缺省值`() {
        val options = Options.parse(arrayOf("adb")).valueOrNull()
        assertNotNull(options)
        assertEquals(TransportKind.ADB, options!!.transport)
        assertEquals("wcreen", options.name)
        assertEquals(27183, options.port)
        assertEquals(LogLevel.VERBOSE, options.logLevel)
    }

    @Test
    fun `tcp 显式端口与日志等级`() {
        val options = Options.parse(arrayOf("tcp", "--port=1234", "--log=W")).valueOrNull()
        assertEquals(1234, options!!.port)
        assertEquals(LogLevel.WARN, options.logLevel)
    }

    @Test
    fun `adb 自定义名字`() {
        val options = Options.parse(arrayOf("adb", "--name=wcreen_2")).valueOrNull()
        assertEquals("wcreen_2", options!!.name)
    }

    @Test
    fun `缺少传输`() {
        val fail = Options.parse(emptyArray()).failOrNull()
        assertEquals(FailCode.BAD_CLI, fail!!.code)
        assertEquals(FailLevel.FATAL, fail.level)
    }

    @Test
    fun `未知传输`() {
        assertEquals(FailCode.BAD_CLI, Options.parse(arrayOf("serial")).failOrNull()!!.code)
    }

    @Test
    fun `端口越界`() {
        assertEquals(FailCode.BAD_CLI, Options.parse(arrayOf("tcp", "--port=0")).failOrNull()!!.code)
        assertEquals(FailCode.BAD_CLI, Options.parse(arrayOf("tcp", "--port=65536")).failOrNull()!!.code)
        assertEquals(FailCode.BAD_CLI, Options.parse(arrayOf("tcp", "--port=abc")).failOrNull()!!.code)
    }

    @Test
    fun `选项与传输不匹配`() {
        // 给了却不生效比报错更糟：用户会以为设上了。
        assertEquals(FailCode.BAD_CLI, Options.parse(arrayOf("tcp", "--name=x")).failOrNull()!!.code)
        assertEquals(FailCode.BAD_CLI, Options.parse(arrayOf("adb", "--port=1234")).failOrNull()!!.code)
    }

    @Test
    fun `未知参数与非法日志等级`() {
        assertEquals(FailCode.BAD_CLI, Options.parse(arrayOf("adb", "--nope")).failOrNull()!!.code)
        assertEquals(FailCode.BAD_CLI, Options.parse(arrayOf("adb", "--log=X")).failOrNull()!!.code)
        assertEquals(FailCode.BAD_CLI, Options.parse(arrayOf("adb", "--name=")).failOrNull()!!.code)
    }

    @Test
    fun `日志等级大小写不敏感`() {
        assertEquals(LogLevel.DEBUG, Options.parse(arrayOf("adb", "--log=d")).valueOrNull()!!.logLevel)
        assertTrue(LogLevel.parse("e") == LogLevel.ERROR)
        assertTrue(LogLevel.parse("Z") == null)
    }
}
