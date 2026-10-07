package com.xiaowine.wcreen.agent.input

import com.xiaowine.wcreen.agent.proto.InputAction
import java.util.ArrayDeque
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.Condition
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

/**
 * `input` 线程的注入队列 —— 实时性的核心（`docs/spec/agent/input.md` §5）。
 *
 * 它不追求"把每条事件都排队处理"，而是让**队列里永远只剩最新状态**：
 * 容量 8 只是上界，真正起作用的是四条合并 / 丢弃规则：
 *
 * | 情形 | 处理 |
 * |---|---|
 * | 队尾是 `move`，又来 `move` | **覆盖** —— 最新的移动覆盖还没处理的移动 |
 * | 队尾是 `scroll`，又来 `scroll` | **累加** `dx` / `dy` —— 滚动是增量语义，覆盖会少滚 |
 * | 满了 | 丢**最旧**的 `move` / `scroll` |
 * | 任何情况 | `down` / `up` / `cancel` / `key` **绝不丢** |
 *
 * 收益：**最坏注入延迟 = 一次注入**（~3 ms），与 PC 送多快无关。
 *
 * ⚠️ 「绝不丢」有个**硬上界**兜底（[HARD_LIMIT]）：注入线程万一被卡住（例如系统输入分发停摆），
 * 不可合并的事件理论上能无限堆积 ⇒ 内存无界。到硬上界时丢**最旧**的一条 ——
 * 与整个队列"只剩最新状态"的哲学一致：要牺牲就牺牲最陈旧的，而不是最新的。
 * 正常路径永远碰不到它（PC 定频采样 ≤ 屏刷新率，而 `input` 线程一次注入 ~3 ms）。
 *
 * 生产者是 `reader`（一条连接一条线程），消费者是 `input` 线程 ⇒ 用一把锁保护。
 * 这不是业务锁，与 `ArrayBlockingQueue` 的内部同步同类。
 *
 * @param capacity 可合并事件（`move` / `scroll`）的容量上界
 */
internal class InputQueue(private val capacity: Int = CAPACITY) {

    /** 队列本体 —— 只在 [lock] 内访问。 */
    private val items: ArrayDeque<InputCmd> = ArrayDeque()

    /** 队列锁。 */
    private val lock = ReentrantLock()

    /** 「队列非空」条件 —— 让 [take] 立刻醒，而不是等满超时。 */
    private val notEmpty: Condition = lock.newCondition()

    /** 被丢掉的 `move` / `scroll` 条数（诊断用）。 */
    var dropped: Long = 0
        private set

    /** 被硬上界挤掉的不可合并事件条数（诊断用；正常恒为 0）。 */
    var evicted: Long = 0
        private set

    /** 当前条数（含控制消息）。 */
    val size: Int get() = lock.withLock { items.size }

    /**
     * 入队 —— **非阻塞**。
     *
     * @param cmd 待入队的一条消息
     */
    fun offer(cmd: InputCmd) {
        lock.withLock {
            if (cmd is InputCmd.Event) {
                // ① 队尾合并：只有 move / scroll 参与
                val tail = items.peekLast()
                if (tail is InputCmd.Event && mergeTail(tail, cmd)) {
                    notEmpty.signalAll()
                    return
                }
                // ② 腾位置：丢最旧的（先丢可合并的，没有才丢最旧的事件）
                while (overLimit()) dropOldest()
            }
            items.addLast(cmd)
            notEmpty.signalAll()
        }
    }

    /**
     * 取一条 —— 阻塞到有货或超时。
     *
     * 用超时而不是永久阻塞：调用方靠"醒来看停止标志"退出，这是全进程统一的收尾手法。
     *
     * @param timeoutMs 最长等待毫秒数
     * @return 队列头；null 表示超时或本线程被中断（两者对调用方等价）
     */
    fun take(timeoutMs: Long): InputCmd? = lock.withLock {
        var remain = TimeUnit.MILLISECONDS.toNanos(timeoutMs)
        while (items.isEmpty() && remain > 0) {
            remain = try {
                notEmpty.awaitNanos(remain)
            } catch (e: InterruptedException) {
                return null
            }
        }
        items.pollFirst()
    }

    /**
     * 队尾合并 —— `move` 覆盖、`scroll` 累加。
     *
     * @param tail 当前队尾（必是事件）
     * @param incoming 新到的事件
     * @return true 表示已被合并（调用方不要再入队）
     */
    private fun mergeTail(tail: InputCmd.Event, incoming: InputCmd.Event): Boolean {
        val tailAction = tail.msg.action
        val newAction = incoming.msg.action
        return when {
            tailAction == InputAction.MOVE && newAction == InputAction.MOVE -> {
                // 覆盖：整条换掉（坐标与 requestId 都取最新）
                items.removeLast()
                items.addLast(incoming)
                true
            }

            tailAction == InputAction.SCROLL && newAction == InputAction.SCROLL -> {
                val into = tail.msg.scroll
                val from = incoming.msg.scroll
                if (into == null || from == null) {
                    false
                } else {
                    into.dx += from.dx
                    into.dy += from.dy
                    // 位置取最新：滚轮跟着指针走
                    into.x = from.x
                    into.y = from.y
                    // 合并后的这一条代表最新的请求 —— 注入失败时按它回报，PC 才对得上号
                    tail.requestId = incoming.requestId
                    true
                }
            }

            else -> false
        }
    }

    /**
     * 是否已到上限。
     *
     * @return true 表示再入队就得先丢最旧的一条
     */
    private fun overLimit(): Boolean =
        mergeableCount() >= capacity || nonMergeableCount() >= HARD_LIMIT

    /**
     * 丢最旧的一条事件 —— 优先丢可合并的（`move` / `scroll`），没有才丢不可合并的。
     *
     * @return false 表示队列里一条事件都没有（只有控制消息）
     */
    private fun dropOldest(): Boolean {
        val mergeableVictim = items.firstOrNull { it is InputCmd.Event && mergeable(it.msg.action) }
        val victim = mergeableVictim ?: items.firstOrNull { it is InputCmd.Event } ?: return false
        items.remove(victim)
        if (victim is InputCmd.Event && mergeable(victim.msg.action)) {
            dropped += 1
        } else {
            evicted += 1
        }
        return true
    }

    /** 可合并（也因此优先被丢）的事件条数。 */
    private fun mergeableCount(): Int =
        items.count { it is InputCmd.Event && mergeable(it.msg.action) }

    /** 不可合并的事件条数 —— 它只有硬上界。 */
    private fun nonMergeableCount(): Int =
        items.count { it is InputCmd.Event && !mergeable(it.msg.action) }

    /**
     * 该 action 是否可合并 / 优先可丢。
     *
     * @param action 事件的 action 串
     * @return true 表示 `move` / `scroll`（丢了不影响状态机）
     */
    private fun mergeable(action: String): Boolean =
        action == InputAction.MOVE || action == InputAction.SCROLL

    /** 容量与上界常量。 */
    companion object {

        /**
         * 可合并事件的容量上界 —— `input.md` §5 定死 8。
         *
         * 依据：一次注入 ~3 ms，8 条足够覆盖"PC 突发一小波、`input` 线程正在注入"的窗口，
         * 再多也只会被合并掉。
         */
        private const val CAPACITY: Int = 8

        /**
         * 不可合并事件的硬上界 —— 防内存无界，正常永远碰不到。
         *
         * 64 = 8 × 容量：比"人手动一下"需要的条数（一个 down、几个按键、一个 up）宽一个量级。
         */
        internal const val HARD_LIMIT: Int = 64
    }
}
