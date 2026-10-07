package com.xiaowine.wcreen.agent.runtime

import java.util.concurrent.atomic.AtomicBoolean

/**
 * 一次性信号。
 *
 * 用来在收尾时单独叫停某条线程 —— 与全局停止标志分开是必要的：`sender` 必须
 * **最后**停（否则收尾消息发不出去），所以它不能跟着全局标志一起退出。
 */
internal class Signal {

    /** 是否已置位。 */
    private val raised = AtomicBoolean(false)

    /** 置位。幂等。 */
    fun raise() {
        raised.set(true)
    }

    /** 是否已置位。 */
    val isRaised: Boolean get() = raised.get()
}
