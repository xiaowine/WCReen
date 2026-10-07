package com.xiaowine.wcreen.agent.runtime

import com.xiaowine.wcreen.agent.transport.Conn
import com.xiaowine.wcreen.agent.transport.Transport
import java.io.IOException

/**
 * 进程级的监听与分派线程。
 *
 * 它只做两件事：阻塞 `accept` → 为每条新连接起一条路。**不做任何解析**。
 *
 * 顺序上它是最后一个起的进程级线程（不变量 5）：它既监听又分派，
 * 而「路由就绪」必须在它开始接受连接之前成立。本实现里每路的队列在 `Route`
 * 构造时就绪，因此 `Route` 一旦构造出来就已具备接消息的能力。
 *
 * @param ln 日志出口
 * @param shutdown 全局停止标志
 * @param coordinator 协调者；新路要登记到它上面
 * @param transport 传输接缝
 * @param deps 进程级共享装配件
 */
internal class AcceptLoop(
    private val ln: Ln,
    private val shutdown: Shutdown,
    private val coordinator: Coordinator,
    private val transport: Transport,
    private val deps: RouteDeps,
) : Thread("accept") {

    /** 下一条路的序号。 */
    private var nextRouteId: Int = 1

    override fun run() {
        try {
            while (!shutdown.stopRequested) {
                val conn = acceptWithRetry() ?: break
                if (shutdown.stopRequested) {
                    // 收尾时用来叫醒 accept 的那次自连接，直接丢掉，不为它起线程。
                    conn.close()
                    break
                }
                val route = Route(nextRouteId++, conn, ln, shutdown, coordinator, deps)
                // 必须在起线程之前登记：心跳定时器只对已登记的路投递。
                coordinator.attachRoute(route)
                route.start()
            }
        } catch (t: Throwable) {
            coordinator.reportFatal(
                Fail(FailLevel.FATAL, FailCode.UNEXPECTED, "accept 线程异常退出", t),
            )
        } finally {
            coordinator.onAcceptFinished()
        }
    }

    /**
     * 接一条连接，瞬时 IO 失败时在同一操作内重试 1 次。
     *
     * @return 新连接；null 表示监听已经结束（收尾 / 连续失败）
     */
    private fun acceptWithRetry(): Conn? {
        try {
            return transport.accept()
        } catch (e: IOException) {
            ln.d("accept 抛 IOException，重试一次：${e.message}")
        }
        return try {
            transport.accept()
        } catch (e: IOException) {
            ln.w("accept 重试仍失败，停止监听：${e.message}")
            null
        }
    }
}
