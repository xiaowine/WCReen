package com.xiaowine.wcreen.agent.runtime

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * 全局停止标志 + 收尾完成闩。
 *
 * 两件事放一起，是因为它们必须**成对**使用才不死锁：`SIGTERM` 的 shutdown hook
 * 等 [awaitTeardown]，而 `main` 的 `finally` 必须在 `System.exit` **之前**
 * 调 [teardownDone] —— 否则 hook 等 main、main 等 hook（`System.exit` 会阻塞
 * 等所有 hook 结束）。
 */
internal class Shutdown {

    /** 停止标志：各线程循环靠它退出。 */
    private val stopFlag = AtomicBoolean(false)

    /** 收尾完成闩：`main` 的 finally 置位，shutdown hook 等它。 */
    private val teardownLatch = CountDownLatch(1)

    /** 是否已请求停止。 */
    val stopRequested: Boolean get() = stopFlag.get()

    /** 请求全局停止。幂等。 */
    fun requestStop() {
        stopFlag.set(true)
    }

    /** 标记收尾已完成 —— 必须在 `System.exit` 之前调用。 */
    fun teardownDone() {
        teardownLatch.countDown()
    }

    /**
     * 等收尾完成。
     *
     * @param timeoutMs 超时毫秒数
     * @return 收尾在超时前完成则 true；false 表示收尾卡住了，调用方不应无限等
     */
    fun awaitTeardown(timeoutMs: Long): Boolean = teardownLatch.await(timeoutMs, TimeUnit.MILLISECONDS)
}
