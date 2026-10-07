package com.xiaowine.wcreen.agent.input

import android.os.SystemClock
import android.view.InputDevice
import android.view.InputEvent
import android.view.KeyCharacterMap
import android.view.KeyEvent
import android.view.MotionEvent
import com.xiaowine.wcreen.agent.proto.ErrorCode
import com.xiaowine.wcreen.agent.proto.InputAction
import com.xiaowine.wcreen.agent.proto.InputArgs
import com.xiaowine.wcreen.agent.proto.KeyMsg
import com.xiaowine.wcreen.agent.proto.ScrollMsg
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome

/**
 * 把协议事件翻译成 Android 的 `InputEvent`。
 *
 * **全部用公开 API 构造**：`MotionEvent.obtain` 的公开重载、`KeyEvent` 的公开构造器
 * ⇒ 事件本体零反射。唯一的隐藏动作是"给事件设目标屏"，它在
 * [`wrappers/InputInjector.kt`](../wrappers/InputInjector.kt)（反射纪律：只在 `wrappers/`）。
 *
 * 无状态 —— 需要跨事件记住的东西（触点 id、`downTime`）都在 [PointerTracker] 里。
 */
internal object EventBuilder {

    /** 触摸压力 —— 触摸屏按下恒为 1.0（注入侧拿不到真实压力，也用不到）。 */
    private const val PRESSURE: Float = 1f

    /** 非物理设备的事件 id；`0` 表示"不是来自物理设备"。 */
    private const val DEVICE_ID: Int = 0

    /** 坐标精度。屏像素即协议坐标 ⇒ 不做亚像素缩放。 */
    private const val PRECISION: Float = 1f

    /**
     * 构造一条待注入事件。
     *
     * @param msg 已解码的输入指令
     * @param tracker 触点跟踪器（触摸类事件会推进它的状态）
     * @return 事件，或**可恢复级**失败（PC 侧给的 action 与载荷对不上）
     */
    fun build(msg: InputArgs, tracker: PointerTracker): Outcome<InputEvent> = when (msg.action) {
        InputAction.SCROLL -> buildScroll(msg.scroll)
        InputAction.KEY_DOWN -> buildKey(msg.key, KeyEvent.ACTION_DOWN)
        InputAction.KEY_UP -> buildKey(msg.key, KeyEvent.ACTION_UP)
        InputAction.DOWN, InputAction.MOVE, InputAction.UP, InputAction.CANCEL ->
            buildTouch(msg, tracker)

        else -> bad("未知的 action '${msg.action}'")
    }

    /**
     * 触摸（含多指）—— `pointerIndex` 与 `downTime` 由 [PointerTracker] 补齐。
     *
     * @param msg 输入指令
     * @param tracker 触点跟踪器
     * @return 事件，或可恢复级失败
     */
    private fun buildTouch(msg: InputArgs, tracker: PointerTracker): Outcome<InputEvent> {
        val now = SystemClock.uptimeMillis()
        val plan = tracker.plan(msg.action, msg.pointers, now)
            ?: return bad("action '${msg.action}' 与触点集合不符（id 变化对不上）")
        if (plan.points.isEmpty()) return bad("触点集合为空（action='${msg.action}'）")

        val count = plan.points.size
        val properties = Array(count) { i ->
            MotionEvent.PointerProperties().also {
                it.id = plan.points[i].id
                it.toolType = MotionEvent.TOOL_TYPE_FINGER
            }
        }
        val coords = Array(count) { i ->
            MotionEvent.PointerCoords().also {
                it.x = plan.points[i].x.toFloat()
                it.y = plan.points[i].y.toFloat()
                it.pressure = PRESSURE
                it.size = 0f
                it.orientation = 0f
            }
        }
        val action = when (plan.kind) {
            PointerTracker.Kind.DOWN -> MotionEvent.ACTION_DOWN

            PointerTracker.Kind.POINTER_DOWN ->
                MotionEvent.ACTION_POINTER_DOWN or (plan.index shl MotionEvent.ACTION_POINTER_INDEX_SHIFT)

            PointerTracker.Kind.MOVE -> MotionEvent.ACTION_MOVE

            PointerTracker.Kind.POINTER_UP ->
                MotionEvent.ACTION_POINTER_UP or (plan.index shl MotionEvent.ACTION_POINTER_INDEX_SHIFT)

            PointerTracker.Kind.UP -> MotionEvent.ACTION_UP
            PointerTracker.Kind.CANCEL -> MotionEvent.ACTION_CANCEL
        }

        // 公开重载（末尾两个参数是 source 与 flags）—— 目标屏由 wrappers 侧的 setDisplayId 设。
        return Outcome.Ok(
            MotionEvent.obtain(
                plan.downTime, now, action, count, properties, coords,
                0, 0, PRECISION, PRECISION, DEVICE_ID, 0,
                InputDevice.SOURCE_TOUCHSCREEN, 0,
            ),
        )
    }

    /**
     * 滚轮 —— 走**鼠标源**且带坐标：App 才会当成滚轮，而不是拖动。
     *
     * @param scroll 滚轮载荷
     * @return 事件，或可恢复级失败（缺载荷）
     */
    private fun buildScroll(scroll: ScrollMsg?): Outcome<InputEvent> {
        if (scroll == null) return bad("action='scroll' 缺少 scroll 字段")
        val now = SystemClock.uptimeMillis()
        val properties = arrayOf(
            MotionEvent.PointerProperties().also {
                it.id = 0
                it.toolType = MotionEvent.TOOL_TYPE_MOUSE
            },
        )
        val coords = arrayOf(
            MotionEvent.PointerCoords().also {
                it.x = scroll.x.toFloat()
                it.y = scroll.y.toFloat()
                it.setAxisValue(MotionEvent.AXIS_HSCROLL, scroll.dx.toFloat())
                it.setAxisValue(MotionEvent.AXIS_VSCROLL, scroll.dy.toFloat())
            },
        )
        return Outcome.Ok(
            MotionEvent.obtain(
                now, now, MotionEvent.ACTION_SCROLL, 1, properties, coords,
                0, 0, PRECISION, PRECISION, DEVICE_ID, 0,
                InputDevice.SOURCE_MOUSE, 0,
            ),
        )
    }

    /**
     * 按键 —— 直接用协议给的 `keyCode` / `metaState`（agent 侧零映射）。
     *
     * `repeat` 恒为 0：长按由 PC 连发 `keyDown` 表达，协议里不引入 repeat 计数。
     *
     * @param key 按键载荷
     * @param action `KeyEvent.ACTION_DOWN` / `ACTION_UP`
     * @return 事件，或可恢复级失败（缺载荷）
     */
    private fun buildKey(key: KeyMsg?, action: Int): Outcome<InputEvent> {
        if (key == null) return bad("action='keyDown' / 'keyUp' 缺少 key 字段")
        val now = SystemClock.uptimeMillis()
        return Outcome.Ok(
            KeyEvent(
                now, now, action, key.code, 0, key.meta,
                KeyCharacterMap.VIRTUAL_KEYBOARD, 0, 0,
                InputDevice.SOURCE_KEYBOARD,
            ),
        )
    }

    /**
     * 造一个可恢复级失败 —— 载荷与 action 对不上属 PC 侧 bug，重试可能成功。
     *
     * @param message 可诊断的说明
     * @return 失败结果
     */
    private fun bad(message: String): Outcome<InputEvent> =
        Outcome.Err(Fail(FailLevel.RECOVERABLE, ErrorCode.BAD_INPUT, message))
}
