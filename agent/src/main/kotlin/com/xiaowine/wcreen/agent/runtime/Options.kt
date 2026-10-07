package com.xiaowine.wcreen.agent.runtime

/** 传输类型 —— CLI 的第一个位置参数，决定「监听什么」。 */
internal enum class TransportKind {

    /** adb 通道：`LocalServerSocket(<抽象名>)`，PC 侧走 `adb forward`。 */
    ADB,

    /** 网络通道：`ServerSocket(<端口>)`，PC 侧直连设备 IP。 */
    TCP,
}

/**
 * 命令行参数。
 *
 * CLI 只放**进程级与调试**参数；业务参数（建屏 / 编码 / 会话策略）一律走协议 ——
 * 见 `runtime-startup.md` §1.1。传输选择是唯一必须启动时给的业务相关参数，
 * 因为它决定「监听什么」，而监听先于任何连接。
 *
 * 只依赖 JDK 类型，不引用任何 Android 类型 ⇒ 可进 JVM 单测。
 *
 * @property transport 传输类型
 * @property name adb 通道的抽象 socket 名（仅 adb 有效）
 * @property port tcp 监听端口（仅 tcp 有效）
 * @property logLevel 启动期日志等级
 */
internal data class Options(
    val transport: TransportKind,
    val name: String,
    val port: Int,
    val logLevel: LogLevel,
) {
    /** 缺省值与解析。 */
    companion object {

        /** adb 抽象 socket 名的缺省值。 */
        const val DEFAULT_NAME: String = "wcreen"

        /** tcp 监听端口的缺省值。 */
        const val DEFAULT_PORT: Int = 27183

        /** `--name=` 前缀。 */
        private const val NAME_PREFIX = "--name="

        /** `--port=` 前缀。 */
        private const val PORT_PREFIX = "--port="

        /** `--log=` 前缀。 */
        private const val LOG_PREFIX = "--log="

        /**
         * 解析命令行。
         *
         * @param args `app_process` 传给入口 `main` 的参数（不含类名）
         * @return 成功给出 [Options]；任何非法输入都返回**启动级**失败（退出码 2），
         *   因为启动阶段唯一的诊断入口就是退出码
         */
        fun parse(args: Array<String>): Outcome<Options> {
            if (args.isEmpty()) return bad("缺少位置参数 <传输>（adb | tcp）")

            val transport = when (args[0]) {
                "adb" -> TransportKind.ADB
                "tcp" -> TransportKind.TCP
                else -> return bad("未知传输 '${args[0]}'，只支持 adb | tcp")
            }

            var name = DEFAULT_NAME
            var port = DEFAULT_PORT
            var logLevel = LogLevel.VERBOSE
            var nameSeen = false
            var portSeen = false

            for (arg in args.drop(1)) {
                when {
                    arg.startsWith(NAME_PREFIX) -> {
                        val value = arg.removePrefix(NAME_PREFIX)
                        if (value.isEmpty()) return bad("--name 不能为空")
                        name = value
                        nameSeen = true
                    }

                    arg.startsWith(PORT_PREFIX) -> {
                        val value = arg.removePrefix(PORT_PREFIX)
                        val parsed = value.toIntOrNull()
                        if (parsed == null || parsed !in 1..65535) {
                            return bad("--port 需要 1..65535 的整数，收到 '$value'")
                        }
                        port = parsed
                        portSeen = true
                    }

                    arg.startsWith(LOG_PREFIX) -> {
                        val value = arg.removePrefix(LOG_PREFIX)
                        logLevel = LogLevel.parse(value) ?: return bad("--log 需要 V|D|I|W|E，收到 '$value'")
                    }

                    else -> return bad("未知参数 '$arg'")
                }
            }

            // 两个选项各自只对一种传输有意义。给错了就当参数错误报出来，
            // 免得用户以为设了却没生效 —— 与「不依赖默认值」同一精神。
            if (transport == TransportKind.TCP && nameSeen) return bad("--name 只对 adb 传输有效")
            if (transport == TransportKind.ADB && portSeen) return bad("--port 只对 tcp 传输有效")

            return Outcome.Ok(Options(transport, name, port, logLevel))
        }

        /**
         * 造一个启动级失败。
         *
         * @param message 可诊断的说明
         * @return 恒为失败
         */
        private fun bad(message: String): Outcome<Options> =
            Outcome.Err(Fail(FailLevel.FATAL, FailCode.BAD_CLI, message))
    }
}
