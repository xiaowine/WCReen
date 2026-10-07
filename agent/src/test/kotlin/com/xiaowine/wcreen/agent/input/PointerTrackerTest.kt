package com.xiaowine.wcreen.agent.input

import com.xiaowine.wcreen.agent.proto.InputAction
import com.xiaowine.wcreen.agent.proto.PointerMsg
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * [PointerTracker] 的 action 推导测试。
 *
 * 它补的是 Android 的 `ACTION_POINTER_DOWN` / `ACTION_POINTER_UP` + `actionIndex`
 * —— 补错的后果是"多指手势被当成单指"或"抬起的手指位置错"，都在真机上手感里才看得出来。
 */
class PointerTrackerTest {

    /** 造一个触点。 */
    private fun point(id: Int, x: Int, y: Int): PointerMsg = PointerMsg(id, x, y)

    @Test
    fun `单指按下是 DOWN 且记下 downTime`() {
        val tracker = PointerTracker()
        val plan = tracker.plan(InputAction.DOWN, listOf(point(7, 100, 200)), 1000)!!
        assertEquals(PointerTracker.Kind.DOWN, plan.kind)
        assertEquals(0, plan.index)
        assertEquals(1000, plan.downTime)
        assertEquals(1, plan.points.size)
    }

    @Test
    fun `手势内共用 downTime 新手势重置`() {
        val tracker = PointerTracker()
        tracker.plan(InputAction.DOWN, listOf(point(0, 1, 1)), 1000)
        assertEquals(1000, tracker.plan(InputAction.MOVE, listOf(point(0, 5, 5)), 1100)!!.downTime)
        tracker.plan(InputAction.UP, emptyList(), 1200)

        // 抬起后用上一份快照定位（坐标是它最后的位置）
        val first = tracker.plan(InputAction.DOWN, listOf(point(1, 2, 2)), 2000)!!
        assertEquals(2000, first.downTime)
        assertEquals(2000, tracker.plan(InputAction.MOVE, listOf(point(1, 3, 3)), 2100)!!.downTime)
    }

    @Test
    fun `单指抬起是 UP 且用最后位置`() {
        val tracker = PointerTracker()
        tracker.plan(InputAction.DOWN, listOf(point(0, 640, 360)), 1000)
        val plan = tracker.plan(InputAction.UP, emptyList(), 1100)!!
        assertEquals(PointerTracker.Kind.UP, plan.kind)
        assertEquals(0, plan.index)
        // 抬起点必须是它最后的位置 —— 空集合里没有任何坐标可用
        assertEquals(640, plan.points[0].x)
        assertEquals(360, plan.points[0].y)
    }

    @Test
    fun `多指按下是 POINTER_DOWN 且 index 指向新指`() {
        val tracker = PointerTracker()
        tracker.plan(InputAction.DOWN, listOf(point(0, 1, 1)), 1000)
        val plan = tracker.plan(InputAction.DOWN, listOf(point(0, 1, 1), point(1, 2, 2)), 1100)!!
        assertEquals(PointerTracker.Kind.POINTER_DOWN, plan.kind)
        assertEquals(1, plan.index)
        // 手势没断，downTime 不变
        assertEquals(1000, plan.downTime)
    }

    @Test
    fun `多指抬起是 POINTER_UP 且被抬的仍在数组里`() {
        val tracker = PointerTracker()
        tracker.plan(InputAction.DOWN, listOf(point(0, 1, 1)), 1000)
        tracker.plan(InputAction.DOWN, listOf(point(0, 1, 1), point(1, 2, 2)), 1100)

        // 抬起 id=1（协议给的是"抬起之后剩下的" ⇒ 只剩 id=0）
        val plan = tracker.plan(InputAction.UP, listOf(point(0, 9, 9)), 1200)!!
        assertEquals(PointerTracker.Kind.POINTER_UP, plan.kind)
        // Android 要求 ACTION_POINTER_UP 里仍带着被抬的那根 ⇒ 补在末尾，index 指它
        assertEquals(2, plan.points.size)
        assertEquals(1, plan.points.lastIndex)
        assertEquals(plan.index, plan.points.lastIndex)
        assertEquals(1, plan.points[plan.index].id)
        // 位置来自上一份快照
        assertEquals(2, plan.points[plan.index].x)
        assertEquals(1000, plan.downTime)
    }

    @Test
    fun `move 原样下发`() {
        val tracker = PointerTracker()
        tracker.plan(InputAction.DOWN, listOf(point(0, 1, 1)), 1000)
        val plan = tracker.plan(InputAction.MOVE, listOf(point(0, 500, 600)), 1050)!!
        assertEquals(PointerTracker.Kind.MOVE, plan.kind)
        assertEquals(500, plan.points[0].x)
        assertEquals(600, plan.points[0].y)
    }

    @Test
    fun `cancel 用上一份快照并结束手势`() {
        val tracker = PointerTracker()
        tracker.plan(InputAction.DOWN, listOf(point(0, 30, 40)), 1000)
        val plan = tracker.plan(InputAction.CANCEL, emptyList(), 1100)!!
        assertEquals(PointerTracker.Kind.CANCEL, plan.kind)
        assertEquals(30, plan.points[0].x)
        // 手势已结束 ⇒ 下一次移动自成一条手势
        assertEquals(2000, tracker.plan(InputAction.MOVE, listOf(point(0, 1, 1)), 2000)!!.downTime)
    }

    @Test
    fun `集合与 action 不符直接拒绝`() {
        val tracker = PointerTracker()
        tracker.plan(InputAction.DOWN, listOf(point(0, 1, 1)), 1000)
        // 多指却没有新 id ⇒ 不能瞎猜 index
        assertNull(tracker.plan(InputAction.DOWN, listOf(point(0, 1, 1), point(0, 2, 2)), 1100))
        // 按下是空集合 ⇒ 无意义
        assertNull(tracker.plan(InputAction.DOWN, emptyList(), 1100))
        // 非触摸 action 不归它管
        assertNull(tracker.plan(InputAction.SCROLL, emptyList(), 1100))
    }
}
