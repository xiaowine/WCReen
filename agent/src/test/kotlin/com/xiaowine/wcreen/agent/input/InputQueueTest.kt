package com.xiaowine.wcreen.agent.input

import com.xiaowine.wcreen.agent.proto.InputAction
import com.xiaowine.wcreen.agent.proto.InputArgs
import com.xiaowine.wcreen.agent.proto.KeyMsg
import com.xiaowine.wcreen.agent.proto.PointerMsg
import com.xiaowine.wcreen.agent.proto.ScrollMsg
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * [InputQueue] 的合并与丢弃测试。
 *
 * 这几条规则是"跟手"的全部依据（`input.md` §5）：覆盖错一条会**少滚 / 丢事件**，
 * 而它们都只在线上高频时才暴露 ⇒ 必须在单测里钉死。
 */
class InputQueueTest {

    /** 造一条触摸 / 滚轮 / 按键事件。 */
    private fun event(action: String, requestId: Int): InputCmd = InputCmd.Event(
        InputArgs(
            action = action,
            pointers = if (action == InputAction.SCROLL) emptyList() else listOf(PointerMsg(0, 10, 20)),
            scroll = if (action == InputAction.SCROLL) ScrollMsg(1, -120, 10, 20) else null,
            key = if (action.startsWith("key")) KeyMsg(4, 0) else null,
        ),
        requestId = requestId,
    )

    /** 取出一条的 requestId。 */
    private fun requestIdOf(cmd: InputCmd?): Int = (cmd as InputCmd.Event).requestId

    @Test
    fun `move 覆盖队尾的 move`() {
        val queue = InputQueue()
        queue.offer(event(InputAction.MOVE, 1))
        queue.offer(event(InputAction.MOVE, 2))
        assertEquals(1, queue.size)
        assertEquals(2, requestIdOf(queue.take(0)))
    }

    @Test
    fun `scroll 累加而不覆盖`() {
        val queue = InputQueue()
        queue.offer(event(InputAction.SCROLL, 1))
        queue.offer(event(InputAction.SCROLL, 2))
        assertEquals(1, queue.size)
        val merged = queue.take(0)
        val scroll = (merged as InputCmd.Event).msg.scroll!!
        // 覆盖会少滚：增量语义必须相加。
        assertEquals(-240, scroll.dy)
        assertEquals(2, scroll.dx)
        // 合并后的这条代表最新请求 ⇒ 失败时才报得对号。
        assertEquals(2, requestIdOf(merged))
    }

    @Test
    fun `不同类型不合并`() {
        val queue = InputQueue()
        queue.offer(event(InputAction.SCROLL, 1))
        queue.offer(event(InputAction.MOVE, 2))
        assertEquals(2, queue.size)
    }

    @Test
    fun `不可合并的事件永不合并`() {
        val queue = InputQueue()
        queue.offer(event(InputAction.DOWN, 1))
        queue.offer(event(InputAction.DOWN, 2))
        queue.offer(event(InputAction.UP, 3))
        assertEquals(3, queue.size)
    }

    @Test
    fun `满了先丢最旧的 move 或 scroll 再放新事件`() {
        val queue = InputQueue()
        repeat(4) {
            queue.offer(event(InputAction.MOVE, it * 2 + 1))
            queue.offer(event(InputAction.SCROLL, it * 2 + 2))
        }
        assertEquals(8, queue.size)

        // 被丢的是**最旧的**，不是新来的
        queue.offer(event(InputAction.MOVE, 99))
        assertEquals(8, queue.size)
        assertEquals(1, queue.dropped)
        assertEquals(2, requestIdOf(queue.take(0)))
    }

    @Test
    fun `不可丢的事件来了就腾位置`() {
        val queue = InputQueue()
        repeat(4) {
            queue.offer(event(InputAction.MOVE, it * 2 + 1))
            queue.offer(event(InputAction.SCROLL, it * 2 + 2))
        }
        assertEquals(8, queue.size)

        // 「满了就丢最旧的 move / scroll」对 `up` 同样成立 —— 绝不因为放不下而丢 up
        queue.offer(event(InputAction.UP, 100))
        assertEquals(8, queue.size)
        assertEquals(1, queue.dropped)
        assertEquals(0, queue.evicted)

        val drained = generateSequence { queue.take(0) }.toList()
        assertEquals(100, requestIdOf(drained.last()))
    }

    @Test
    fun `不可合并的事件也有硬上界`() {
        val queue = InputQueue()
        // 注入线程万一被卡住，down / key 这类事件理论上能无限堆积 ⇒ 必须有硬上界兜底
        repeat(InputQueue.HARD_LIMIT) {
            queue.offer(event(InputAction.DOWN, it + 1))
        }
        assertEquals(InputQueue.HARD_LIMIT, queue.size)

        queue.offer(event(InputAction.UP, 999))
        // 丢的是**最旧的**（要牺牲就牺牲最陈旧的），所以最新那条还在
        assertEquals(InputQueue.HARD_LIMIT, queue.size)
        assertEquals(1, queue.evicted)
        val drained = generateSequence { queue.take(0) }.toList()
        assertEquals(999, requestIdOf(drained.last()))
        assertEquals(2, requestIdOf(drained.first()))
    }

    @Test
    fun `先到先出且能唤醒`() {
        val queue = InputQueue()
        queue.offer(event(InputAction.DOWN, 1))
        queue.offer(event(InputAction.MOVE, 2))
        assertEquals(1, requestIdOf(queue.take(1000)))
        assertEquals(2, requestIdOf(queue.take(1000)))
    }

    @Test
    fun `空队列取超时返回 null`() {
        assertNull(InputQueue().take(20))
    }

    @Test
    fun `目标屏变化不占容量`() {
        val queue = InputQueue()
        // 8 条可合并事件（正好占满容量）+ 4 条目标屏变化
        repeat(4) {
            queue.offer(event(InputAction.MOVE, it * 2 + 1))
            queue.offer(InputCmd.ScreenChanged(7))
            queue.offer(event(InputAction.SCROLL, it * 2 + 2))
        }
        assertEquals(12, queue.size)
        // 控制消息从不因容量被丢，也不用挤掉输入事件
        assertEquals(0, queue.dropped)
        assertNotNull(queue.take(0))
    }
}
