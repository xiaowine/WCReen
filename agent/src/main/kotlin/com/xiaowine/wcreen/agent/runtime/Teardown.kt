package com.xiaowine.wcreen.agent.runtime

import com.xiaowine.wcreen.agent.transport.Transport

/**
 * 全局收尾序列 —— `main` 唯一编排的序列。
 *
 * 顺序是硬的，每一步都在解开一处阻塞：
 *
 * ```
 * ① 设停止标志 + `wake()` 叫醒 `accept` → join → 关监听 socket
 * ② 每路拆读侧 ⇒ reader 退出（写侧仍可用）
 * ③ 每路停 input ⇒ join（它没有要排空的东西：连接没了，队列里剩的输入就作废）
 * ④ 每路等 link 退出 ⇒ 它在 finally 里按硬顺序释放屏与编码器（⑤）
 * ⑥ 每路：sender 排空队列 → 拆写侧 → 退出
 * ⑦ 传输关闭（①已做，幂等）
 * ```
 *
 * `sender` 放在最后停，是为了让收尾消息还能发出去 —— 先关 socket 就全丢了。
 * `shutdownInput()` / `shutdownOutput()` 分别拆掉读侧与写侧，让阻塞的读 / 写立刻返回，
 * 同时另一侧还能用。
 */
internal object Teardown {

    /** 每个组件 join 的超时；超时就继续往下，不阻塞收尾。 */
    private const val JOIN_TIMEOUT_MS: Long = 3_000

    /** `sender` 排空队列的超时。 */
    private const val DRAIN_TIMEOUT_MS: Long = 1_000

    /**
     * 跑完整个收尾序列。
     *
     * @param ln 日志出口
     * @param shutdown 全局停止标志
     * @param coordinator 协调者；提供活跃路列表与 accept 线程
     * @param transport 传输；先关它才能让 accept 从阻塞中返回
     */
    fun run(ln: Ln, shutdown: Shutdown, coordinator: Coordinator, transport: Transport) {
        val routes = coordinator.liveRoutes()
        ln.i("收尾开始：${routes.size} 条路")

        // ① 停止标志 + 叫醒阻塞中的 accept，再关监听 ⇒ accept 退出
        //    顺序不能换：close() 不会让 accept() 返回，先关就没人叫得醒它了。
        shutdown.requestStop()
        transport.wake()
        coordinator.acceptThread()?.join(JOIN_TIMEOUT_MS)
        transport.close()

        // ② 先拆读侧：reader 立刻从阻塞的读返回，而写侧仍可用
        for (route in routes) route.shutdownInput()
        for (route in routes) route.joinReader(JOIN_TIMEOUT_MS)

        // ③ 停 input：它没有要排空的东西（连接没了，队列里剩的输入就作废），
        //    且必须早于 ④ —— 否则它可能往一块已经拆掉的屏上注入。
        for (route in routes) route.stopInput()
        for (route in routes) route.joinInput(JOIN_TIMEOUT_MS)

        // ④ 停显示链路：它的 dequeue 是短超时，循环会看到停止标志后退出，
        //    并在自己的 finally 里按硬顺序释放屏与编码器（⑤）。
        for (route in routes) route.joinLink(JOIN_TIMEOUT_MS)

        // ⑥ 最后停 sender：先让它把队列排空，再拆写侧
        for (route in routes) route.stopSender()
        for (route in routes) route.joinSender(DRAIN_TIMEOUT_MS + JOIN_TIMEOUT_MS)
        for (route in routes) route.shutdownOutput()
        for (route in routes) route.joinSender(JOIN_TIMEOUT_MS)

        ln.i("收尾完成")
    }
}
