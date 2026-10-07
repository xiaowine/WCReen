package com.xiaowine.wcreen.agent.runtime

import android.util.Log

/**
 * 日志等级。
 *
 * 自建枚举而不是直接用 `android.util.Log` 的常量，是为了让 CLI 解析不引用 Android 类型
 * （`code.md` §8 的可测性分层）。等级取值沿用 Android 原生那套，运行期可以用
 * `setprop log.tag.WCReen <LEVEL>` 覆盖，零代码。
 */
internal enum class LogLevel {
    VERBOSE,
    DEBUG,
    INFO,
    WARN,
    ERROR;

    /** 映射到 `android.util.Log` 的等级常量。 */
    fun androidLevel(): Int = when (this) {
        VERBOSE -> Log.VERBOSE
        DEBUG -> Log.DEBUG
        INFO -> Log.INFO
        WARN -> Log.WARN
        ERROR -> Log.ERROR
    }

    /** 解析与缺省值。 */
    companion object {

        /** 按 CLI 的单个字母解析；不识别时返回 null（由调用方给出可诊断的报错）。 */
        fun parse(letter: String): LogLevel? = when (letter.uppercase()) {
            "V" -> VERBOSE
            "D" -> DEBUG
            "I" -> INFO
            "W" -> WARN
            "E" -> ERROR
            else -> null
        }
    }
}

/**
 * 全模块统一的日志出口。
 *
 * 为什么不直接调 `android.util.Log`：一是要**自动附线程名**（线程表是固定角色，
 * 日志里没有线程名就没法定位是哪条路、哪一环）；二是要**运行期能改等级**。
 *
 * 通道就是 logcat：不落盘、不走 stdout（脱离 adb 启动后 stdout 被丢弃），
 * 因此连接建立之前的日志天然也能看到。
 *
 * 实例由 `Main` 创建、按构造参数注入各组件，不做全局可变单例（`code.md` §6）。
 *
 * @property level 当前等级；启动期由 `--log` 给定，运行期可改。缺省 VERBOSE
 *   （默认 `isLoggable(V/D)` 是 false，但实测仍会进 logcat；要稳定可见就
 *   `setprop log.tag.WCReen D`）
 */
internal class Ln(level: LogLevel) {

    /** 当前等级 —— 低于它的日志直接丢弃。 */
    @Volatile
    var level: LogLevel = level

    /** 记一条 VERBOSE 日志。 */
    fun v(msg: String) = write(Log.VERBOSE, LogLevel.VERBOSE, msg)

    /** 记一条 DEBUG 日志。 */
    fun d(msg: String) = write(Log.DEBUG, LogLevel.DEBUG, msg)

    /** 记一条 INFO 日志。 */
    fun i(msg: String) = write(Log.INFO, LogLevel.INFO, msg)

    /** 记一条 WARN 日志。 */
    fun w(msg: String) = write(Log.WARN, LogLevel.WARN, msg)

    /** 记一条 ERROR 日志。 */
    fun e(msg: String) = write(Log.ERROR, LogLevel.ERROR, msg)

    /**
     * 记一条带异常堆栈的 ERROR 日志。
     *
     * @param msg 说明
     * @param t 异常；用 `Log.getStackTraceString` 取堆栈，避免 logcat 的异常分帧被截断
     */
    fun e(msg: String, t: Throwable) = write(Log.ERROR, LogLevel.ERROR, "$msg\n${Log.getStackTraceString(t)}")

    /**
     * 等级够了才真正写 —— 过滤在进程内先做一次，省掉热路径上的字符串拼接。
     *
     * @param androidLevel 传给 logd 的等级
     * @param required 本条日志要求的等级
     * @param msg 日志正文
     */
    private fun write(androidLevel: Int, required: LogLevel, msg: String) {
        if (level.ordinal > required.ordinal) return
        Log.println(androidLevel, TAG, "[${Thread.currentThread().name}] $msg")
    }

    /** 固定 tag。 */
    companion object {

        /** 固定 tag —— 便于 `logcat -s WCReen`。 */
        const val TAG: String = "WCReen"
    }
}
