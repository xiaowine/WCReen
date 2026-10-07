package com.xiaowine.wcreen.agent

import android.os.Handler
import android.os.Looper
import android.view.Display
import com.xiaowine.wcreen.agent.display.Bootstrap
import com.xiaowine.wcreen.agent.display.CompanionAssociation
import com.xiaowine.wcreen.agent.encode.CodecCaps
import com.xiaowine.wcreen.agent.runtime.AcceptLoop
import com.xiaowine.wcreen.agent.runtime.Coordinator
import com.xiaowine.wcreen.agent.runtime.ExitCode
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Ln
import com.xiaowine.wcreen.agent.runtime.LogLevel
import com.xiaowine.wcreen.agent.runtime.Options
import com.xiaowine.wcreen.agent.runtime.Outcome
import com.xiaowine.wcreen.agent.runtime.RouteDeps
import com.xiaowine.wcreen.agent.runtime.Shutdown
import com.xiaowine.wcreen.agent.runtime.Teardown
import com.xiaowine.wcreen.agent.runtime.TransportKind
import com.xiaowine.wcreen.agent.transport.AdbTransport
import com.xiaowine.wcreen.agent.transport.TcpTransport
import com.xiaowine.wcreen.agent.transport.Transport
import com.xiaowine.wcreen.agent.wrappers.DisplayManagerFactory
import com.xiaowine.wcreen.agent.wrappers.ActivitySizeRestrictions
import com.xiaowine.wcreen.agent.wrappers.InputInjector
import com.xiaowine.wcreen.agent.wrappers.VirtualDevice
import com.xiaowine.wcreen.agent.wrappers.VirtualDisplayFlags
import java.io.IOException
import kotlin.system.exitProcess

/**
 * agent 入口 —— `app_process` 按**类名字符串**加载本类，因此类名与包名是对外契约，
 * 改名即废掉启动脚本（`module.md` §4）。
 *
 * 启动编排六步（`runtime-startup.md` §1）：
 * ①异常兜底 → ②进程引导 → ③解析 CLI → ④建队列与共享状态 → ⑤起进程级线程（accept 最后）
 * → ⑥`Looper.loop()`；收尾在 `finally` 里跑，最后显式 `System.exit`。
 */
object Main {

    /** 终止信号后等收尾完成的超时 —— 与收尾总上限同量级。 */
    private const val SIGNAL_TEARDOWN_TIMEOUT_MS: Long = 3_000

    /** 虚拟设备名 —— 只作诊断用（会出现在 `dumpsys display` 里）。 */
    private const val DEVICE_NAME: String = "WCReen"

    /**
     * 入口。
     *
     * @param args `app_process` 传入的参数（不含类名）：`<传输> [选项]`
     */
    @JvmStatic
    fun main(args: Array<String>) {
        // ① 异常兜底最先装：进程引导本身会抛「看不懂的异常」，那时还没有协调者可汇报。
        val ln = Ln(LogLevel.VERBOSE)
        installBootstrapFatalHandler(ln)

        val status = run(args, ln)
        ln.i("进程退出，退出码 $status")
        System.exit(status)
    }

    /**
     * 第一段兜底 —— **引导之前**的未捕获异常处理。
     *
     * 此刻还没有协调者、没有任何组件线程、也没有需要收尾的资源，所以直接 `halt`：
     * 它不跑 shutdown hook，是"确实没有东西可收尾"时的正确选择。
     *
     * @param ln 日志出口
     */
    private fun installBootstrapFatalHandler(ln: Ln) {
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            ln.e("线程 ${t.name} 未捕获异常（引导阶段）", e)
            Runtime.getRuntime().halt(ExitCode.RUNTIME_FATAL)
        }
    }

    /**
     * 第二段兜底 —— 协调者就位后**替换**掉第一段。
     *
     * 组件线程意外退出是**致命来源之一**（R2：不做"部分存活"模式）⇒ 必须走收尾序列
     * （释放屏与编码器、退出码 1），不能硬停。
     *
     * 之所以要两段而不是"装一次 + 查一个可变的协调者引用"：后者需要一个**可变静态字段**，
     * 与 `code.md` §6「无全局可变单例」冲突。两段之间（②③④）只有 main 一条线程，
     * 且 main 的异常由 [run] 的 `try/catch` 兜住 ⇒ 没有空窗。
     *
     * @param ln 日志出口
     * @param coordinator 已就位的协调者
     */
    private fun installRuntimeFatalHandler(ln: Ln, coordinator: Coordinator) {
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            ln.e("线程 ${t.name} 未捕获异常", e)
            coordinator.reportFatal(
                Fail(FailLevel.FATAL, FailCode.UNEXPECTED, "线程 ${t.name} 未捕获异常", e),
            )
        }
    }

    /**
     * 六步编排与主线程骨架。
     *
     * @param args 命令行参数
     * @param ln 日志出口
     * @return 退出码：`0` 正常结束 / `1` 运行中致命错误 / `2` 启动失败
     */
    private fun run(args: Array<String>, ln: Ln): Int {
        var status: Int
        var shutdown: Shutdown? = null
        var coordinator: Coordinator? = null
        var transport: Transport? = null

        try {
            // ② 进程引导：主线程 Looper + 伪装包名的系统 Context。必须早于任何线程。
            val context = when (val r = Bootstrap.run(ln)) {
                is Outcome.Ok -> r.value
                is Outcome.Err -> {
                    ln.e(r.fail.toString())
                    if (r.fail.cause != null) ln.e(r.fail.message, r.fail.cause)
                    return ExitCode.STARTUP_FAILURE
                }
            }

            // ③ 解析 CLI 参数
            val options = when (val r = Options.parse(args)) {
                is Outcome.Ok -> r.value
                is Outcome.Err -> {
                    ln.e(r.fail.toString())
                    return ExitCode.STARTUP_FAILURE
                }
            }
            ln.level = options.logLevel

            // ④ 建队列与共享状态 + 构建能力表（设备级静态量，全进程不变）。
            //    反射成员与能力都在这里一次性解析：解析失败 = 启动失败（退出码 2），
            //    把"用到才发现"前移成"启动就发现、一次看全"。
            val handler = Handler(Looper.getMainLooper())
            val sd = Shutdown()
            shutdown = sd
            val coord = Coordinator(ln, sd, handler)
            coordinator = coord

            // 兜底升级：协调者已就位 ⇒ 未捕获异常改走收尾（退出码 1），不再硬停。
            installRuntimeFatalHandler(ln, coord)

            for (resolve in listOf(
                VirtualDisplayFlags::resolve,
                DisplayManagerFactory::resolve,
                VirtualDevice::resolve,
                InputInjector::resolve,
            )) {
                when (val r = resolve()) {
                    is Outcome.Ok -> Unit
                    is Outcome.Err -> {
                        ln.e(r.fail.toString())
                        if (r.fail.cause != null) ln.e(r.fail.message, r.fail.cause)
                        return ExitCode.STARTUP_FAILURE
                    }
                }
            }

            // 「忽略活动尺寸限制」：**可降级**，故不进上面那张 fatal 列表 —— 拿不到（Android 14/15）
            // 就回到平台默认行为（这类 App 可能被裁）。
            if (!ActivitySizeRestrictions.resolve()) {
                ln.w(
                    "本机没有 setIgnoreActivitySizeRestrictions（Android 16 起才有）⇒ 活动级 letterbox 无法取消"
                        + "（不可缩放 + 固定方向的 App 可能被裁）"
                )
            }

            val caps = when (val r = CodecCaps.probe()) {
                is Outcome.Ok -> r.value
                is Outcome.Err -> {
                    // 没有硬编 ⇒ 建屏无从谈起；这是子系统级，但在启动阶段没有可服务的会话
                    // ⇒ 按启动失败处理，退出码 2（PC 会立刻知道设备不可用）。
                    ln.e(r.fail.toString())
                    return ExitCode.STARTUP_FAILURE
                }
            }

            val displayManager = when (val r = DisplayManagerFactory.create(context)) {
                is Outcome.Ok -> r.value
                is Outcome.Err -> {
                    ln.e(r.fail.toString())
                    if (r.fail.cause != null) ln.e(r.fail.message, r.fail.cause)
                    return ExitCode.STARTUP_FAILURE
                }
            }

            // 注入器与 DisplayManager 两个都从伪装 Context 取；注入只看调用进程 uid，
            // 所以它不需要隐藏构造器（见 wrappers/InputInjector.kt 的取法说明）。
            val inputInjector = when (val r = InputInjector.create(context)) {
                is Outcome.Ok -> r.value
                is Outcome.Err -> {
                    ln.e(r.fail.toString())
                    if (r.fail.cause != null) ln.e(r.fail.message, r.fail.cause)
                    return ExitCode.STARTUP_FAILURE
                }
            }

            // 虚拟设备：屏挂在它上面（"锁屏/息屏不接管"与"铺满"同时成立的唯一路径）。
            // 两个前置都在这条路径上：CDM 关联（缺则建，幂等）+ attribution 伪装（在引导里做）。
            val associationId = when (val r = CompanionAssociation.ensure(ln)) {
                is Outcome.Ok -> r.value
                is Outcome.Err -> {
                    ln.e(r.fail.toString())
                    if (r.fail.cause != null) ln.e(r.fail.message, r.fail.cause)
                    return ExitCode.STARTUP_FAILURE
                }
            }
            val device = when (val r = VirtualDevice.create(
                context = context,
                associationId = associationId,
                name = DEVICE_NAME,
                alwaysUnlocked = true,
            )) {
                is Outcome.Ok -> r.value
                is Outcome.Err -> {
                    ln.e(r.fail.toString())
                    if (r.fail.cause != null) ln.e(r.fail.message, r.fail.cause)
                    return ExitCode.STARTUP_FAILURE
                }
            }
            ln.i("虚拟设备已就绪：$DEVICE_NAME（关联号 $associationId）")

            // 缺省尺寸 / 密度 = 主屏（绝不写死常量）。
            val mainDisplay = displayManager.getDisplay(Display.DEFAULT_DISPLAY)
            if (mainDisplay == null) {
                ln.e("拿不到主屏，无法推导缺省尺寸")
                return ExitCode.STARTUP_FAILURE
            }
            // 缺省 DPI 取主屏密度。`Display.getMetrics()` 已废弃，而 `Display` 没有
            // 非废弃的密度入口 ⇒ 用资源里的 `DisplayMetrics`（默认屏就是主屏）。
            val deps = RouteDeps(
                virtualDevice = device,
                flags = VirtualDisplayFlags.get(),
                caps = caps,
                defaultWidth = mainDisplay.mode.physicalWidth,
                defaultHeight = mainDisplay.mode.physicalHeight,
                defaultDpi = context.resources.displayMetrics.densityDpi,
                inputInjector = inputInjector,
            )
            ln.i(
                "能力表已就绪：${caps.summary()}；" +
                    "主屏 ${deps.defaultWidth}x${deps.defaultHeight}@${deps.defaultDpi}dpi",
            )

            // 终止信号：SIGTERM / SIGINT 都走 shutdown hook（SIGKILL 接管不了）。
            // hook 只「请求停止 + 等收尾」，真正的收尾在 main 的 finally 里 ——
            // 顺序必须是 finally 先 teardownDone() 再 System.exit()，否则两边互相等。
            Runtime.getRuntime().addShutdownHook(
                Thread({
                    ln.i("收到终止信号 ⇒ 请求收尾")
                    sd.requestStop()
                    handler.post { Looper.getMainLooper().quitSafely() }
                    if (!sd.awaitTeardown(SIGNAL_TEARDOWN_TIMEOUT_MS)) {
                        ln.w("收尾在 $SIGNAL_TEARDOWN_TIMEOUT_MS ms 内没完成，hook 不再等")
                    }
                }, "shutdown-hook"),
            )

            // ⑤ 起进程级线程：accept 必须最后（不变量 5 —— 它既监听又分派，
            //    起它之前路由必须已经就绪）。绑定失败 ⇒ 启动失败 ⇒ 退出码 2，
            //    「重复实例」也是靠这条路径天然判出的。
            val t = try {
                createTransport(options, ln)
            } catch (e: IOException) {
                ln.e("监听失败（${describe(options)}）：${e.message}", e)
                return ExitCode.STARTUP_FAILURE
            }
            transport = t
            val accept = AcceptLoop(ln, sd, coord, t, deps)
            coord.attachAccept(accept)
            accept.start()
            coord.startHeartbeat()
            ln.i("已监听：${describe(options)}")

            // ⑥ Looper.loop() —— 被「全部组件结束」「显式停止」或「任一 fatal」打断。
            Looper.loop()
            status = coord.exitStatus
        } catch (t: Throwable) {
            // main 自身异常是致命来源之一。
            ln.e("main 异常", t)
            status = ExitCode.RUNTIME_FATAL
        } finally {
            val sd = shutdown
            val coord = coordinator
            val t = transport
            if (sd != null && coord != null && t != null) {
                try {
                    Teardown.run(ln, sd, coord, t)
                } catch (e: Throwable) {
                    ln.e("收尾序列异常", e)
                }
            }
            // 必须在 System.exit 之前 —— 否则 shutdown hook 会一直等下去。
            sd?.teardownDone()
        }
        return status
    }

    /**
     * 按传输类型造监听。
     *
     * @param options 已解析的命令行
     * @param ln 日志出口
     * @return 已绑定的传输
     * @throws IOException 绑定失败（地址被占用 / 权限不足）
     */
    private fun createTransport(options: Options, ln: Ln): Transport = when (options.transport) {
        TransportKind.ADB -> AdbTransport(options.name, ln)
        TransportKind.TCP -> TcpTransport(options.port, ln)
    }

    /**
     * 给人看的监听描述。
     *
     * @param options 已解析的命令行
     * @return 例如 `adb 抽象名 'wcreen'`
     */
    private fun describe(options: Options): String = when (options.transport) {
        TransportKind.ADB -> "adb 抽象名 '${options.name}'"
        TransportKind.TCP -> "tcp 端口 ${options.port}"
    }
}
