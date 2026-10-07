package com.xiaowine.wcreen.agent.runtime

import android.os.Handler
import android.os.Looper
import com.xiaowine.wcreen.agent.proto.Codec
import com.xiaowine.wcreen.agent.proto.HeartbeatMsg
import com.xiaowine.wcreen.agent.proto.Timing
import com.xiaowine.wcreen.agent.proto.Wire
import java.util.concurrent.ConcurrentHashMap

/**
 * 协调者 —— 只持**跨子系统的策略与终局**，不镜像任何子系统的状态。
 *
 * 不镜像状态是硬要求：协调者若也存一份屏 / 编码器 / 连接的状态，就有了第二份真相，
 * 必然漂移。它只做三件事：聚合终止条件、持跨子系统策略、发起全局收尾。
 *
 * 本类的方法可从任意线程调用 —— 所有状态改动都转投 main 的 `Handler`。
 *
 * @param ln 日志出口
 * @param shutdown 全局停止标志
 * @param handler main 线程的 Handler
 */
internal class Coordinator(
    private val ln: Ln,
    private val shutdown: Shutdown,
    private val handler: Handler,
) {

    /**
     * 心跳帧 —— 负载固定，预编码一次后每 3s 直接复用。
     * 写出去不会修改字节数组，所以多路共享同一份是安全的。
     */
    private val heartbeatFrame: Wire.Frame =
        Wire.Frame(Wire.TYPE_HEARTBEAT, Codec.encode(Wire.TYPE_HEARTBEAT, HeartbeatMsg()))

    /**
     * 活跃的路。
     *
     * 用并发集合而不是加锁：`accept` 线程要登记、main 要遍历、连接结束时要移除，
     * 三处跨线程。这与 `ArrayBlockingQueue` 内部同步是同一类做法，不是业务锁。
     */
    private val routes: MutableSet<Route> = ConcurrentHashMap.newKeySet()

    /** `accept` 线程 —— 收尾时 join 它。只在 main 线程读写。 */
    private var accept: Thread? = null

    /** 退出码 —— 任一 fatal 会把它抬成非 0。只在 main 线程读写。 */
    private var status: Int = ExitCode.OK

    /** 是否已经请求退出 `Looper.loop()`。 */
    private var quitRequested: Boolean = false

    /** 心跳定时器 —— 跑在 main 的 Handler 上，零新线程。 */
    private val heartbeatTask: Runnable = object : Runnable {
        override fun run() {
            for (route in routes) {
                if (!route.offer(heartbeatFrame)) {
                    ln.w("路 #${route.id} 控制队列已满，本条心跳丢弃")
                }
            }
            handler.postDelayed(this, Timing.HEARTBEAT_PERIOD_MS)
        }
    }

    /** 当前退出码。 */
    val exitStatus: Int get() = status

    /** 当前活跃的路快照 —— 收尾序列按它遍历。 */
    fun liveRoutes(): List<Route> = routes.toList()

    /** 登记 `accept` 线程。 */
    fun attachAccept(thread: Thread) {
        accept = thread
    }

    /** `accept` 线程 —— 收尾序列要 join 它。 */
    fun acceptThread(): Thread? = accept

    /** 启动心跳定时器。 */
    fun startHeartbeat() {
        handler.postDelayed(heartbeatTask, Timing.HEARTBEAT_PERIOD_MS)
    }

    /**
     * 登记一条新接入的路。
     *
     * 必须在起该路的线程**之前**调用：心跳定时器只对已登记的路投递。
     *
     * @param route 新路
     */
    fun attachRoute(route: Route) {
        routes.add(route)
    }

    /**
     * 一条路结束（连接断开 / 判死 / 协议错误）。
     *
     * @param route 已结束的路
     */
    fun onRouteFinished(route: Route) {
        routes.remove(route)
        handler.post { maybeQuit() }
    }

    /** `accept` 线程自己结束了（不是被收尾停掉的）。 */
    fun onAcceptFinished() {
        handler.post { maybeQuit() }
    }

    /**
     * 上报致命失败 —— 可从任意线程调用。
     *
     * @param fail 级别应为 [FailLevel.FATAL]
     */
    fun reportFatal(fail: Fail) {
        ln.e("fatal：$fail")
        if (fail.cause != null) ln.e(fail.message, fail.cause)
        handler.post {
            status = ExitCode.RUNTIME_FATAL
            quit()
        }
    }

    /**
     * 请求全局停止。
     *
     * @param status 退出码；正常停止给 [ExitCode.OK]
     */
    fun requestStop(status: Int) {
        handler.post {
            this.status = status
            shutdown.requestStop()
            quit()
        }
    }

    /**
     * 全部组件都结束了 ⇒ 退出 `Looper.loop()`，由 `Main` 的 `finally` 走收尾。
     */
    private fun maybeQuit() {
        val acceptThread = accept
        if (acceptThread == null || acceptThread.isAlive) return
        if (routes.isNotEmpty()) return
        quit()
    }

    /** 请求退出主循环。幂等 —— 重复调用只会让 `quitSafely` 抛异常，没有意义。 */
    private fun quit() {
        if (quitRequested) return
        quitRequested = true
        Looper.getMainLooper().quitSafely()
    }
}
