package com.xiaowine.wcreen.agent.input

import com.xiaowine.wcreen.agent.proto.InputAction
import com.xiaowine.wcreen.agent.proto.PointerMsg

/**
 * 触点快照跟踪器 —— 由"上一组触点 id"推导 Android 的 `pointerIndex`（`input.md` §2）。
 *
 * 协议只给粗粒度 action（`down` / `move` / `up` / `cancel`），而 Android 要
 * `ACTION_POINTER_DOWN` / `ACTION_POINTER_UP` + `actionIndex`。补齐在这一层完成
 * ⇒ **PC 不需要懂 Android 的 action 细节**，协议与平台解耦。
 *
 * 三类需要推导的信息：
 *  1. **多指的第几个**（`actionIndex`）；
 *  2. **被抬起的触点要留在事件数组里**（Android 要求 `ACTION_POINTER_UP` / `ACTION_UP`
 *     的事件里仍带着那根手指）—— 靠上一份快照补它最后的位置；
 *  3. **手势的 `downTime`**（整条手势共用按下时刻，不是每条事件各自的时刻）。
 *
 * 约定（`input.md` §2 的精确化）：`pointers` 是**变更后**的当前触点集合
 * —— `up` 给的是"抬起之后剩下的"，因此被抬的那根要由本类补回。
 *
 * 纯逻辑、不碰 Android 类型 ⇒ 可进 `agent/src/test/`。**只在 `input` 线程使用**，单线程无锁。
 */
internal class PointerTracker {

    /** 上一次下发过的触点快照。 */
    private var last: List<PointerMsg> = emptyList()

    /** 当前手势的按下时刻（毫秒）；无手势时无意义。 */
    private var downTime: Long = 0

    /** 当前是否有一条手势在进行。 */
    private var active: Boolean = false

    /**
     * 一次注入要用的动作。
     *
     * @property kind 动作种类 —— 与 Android `ACTION_*` 一一对应，映射在 `EventBuilder`
     * @property index `actionIndex`：多指时"被改动的那一根"在 [points] 里的下标
     * @property points 本次事件要下发的触点数组，顺序即 `pointerIndex` 的顺序
     * @property downTime 本次事件该用的按下时刻（整条手势共用一个）
     */
    class Plan(
        val kind: Kind,
        val index: Int,
        val points: List<PointerMsg>,
        val downTime: Long,
    )

    /** 动作种类 —— 与协议 action 是多对一的（多指时会细分）。 */
    enum class Kind {

        /** 首个触点按下。 */
        DOWN,

        /** 已有触点时又按下了一根。 */
        POINTER_DOWN,

        /** 移动。 */
        MOVE,

        /** 多指时抬起了其中一根。 */
        POINTER_UP,

        /** 最后一个触点抬起。 */
        UP,

        /** 手势取消。 */
        CANCEL,
    }

    /**
     * 推导一条触摸事件的动作，并推进内部状态。
     *
     * @param action 协议 action：`down` / `move` / `up` / `cancel`
     * @param incoming **变更后**的当前触点集合
     * @param now 当前时刻（毫秒）；由调用方给，保持本类纯函数
     * @return 本次要下发的动作；null 表示 `action` 与触点集合对不上（PC 侧 bug）
     */
    fun plan(action: String, incoming: List<PointerMsg>, now: Long): Plan? = when (action) {
        InputAction.DOWN -> planDown(incoming, now)
        InputAction.MOVE -> {
            startIfIdle(now)
            last = incoming
            Plan(Kind.MOVE, 0, incoming, downTime)
        }

        InputAction.UP -> planUp(incoming, now)
        InputAction.CANCEL -> {
            val plan = Plan(Kind.CANCEL, 0, last.ifEmpty { incoming }, effectiveDownTime(now))
            last = emptyList()
            active = false
            plan
        }

        else -> null
    }

    /**
     * 按下：首根是 `ACTION_DOWN`，其后是 `ACTION_POINTER_DOWN`。
     *
     * 多根同时变化时按**第一个**新 id 推导（其余按同一事件带入）—— 容忍而不报错，
     * 因为一次只变一根才是 PC 的正常行为。
     *
     * @param incoming 变更后的触点集合（含刚按下的那根）
     * @param now 当前时刻
     * @return 动作，或 null（多指却没有新 id ⇒ 集合与 action 不符）
     */
    private fun planDown(incoming: List<PointerMsg>, now: Long): Plan? {
        if (incoming.isEmpty()) return null
        if (incoming.size == 1) {
            downTime = now
            active = true
            last = incoming
            return Plan(Kind.DOWN, 0, incoming, downTime)
        }
        val added = incoming.firstOrNull { p -> last.none { it.id == p.id } } ?: return null
        startIfIdle(now)
        last = incoming
        return Plan(Kind.POINTER_DOWN, incoming.indexOf(added), incoming, downTime)
    }

    /**
     * 抬起：最后一根是 `ACTION_UP`，多指抬起是 `ACTION_POINTER_UP`。
     *
     * 被抬的那根**必须留在事件数组里**（Android 的约定），所以从上一份快照把它的位置补回来。
     *
     * @param incoming 变更后的触点集合（**已不含**被抬起的那根）
     * @param now 当前时刻
     * @return 动作，或 null（多指却找不出被抬的是哪根 ⇒ 集合与 action 不符）
     */
    private fun planUp(incoming: List<PointerMsg>, now: Long): Plan? {
        val lifted = last.firstOrNull { p -> incoming.none { it.id == p.id } }
        val base = effectiveDownTime(now)

        // 单指（或没有历史）：抬起点就是它最后的位置
        if (last.size <= 1) {
            val points = if (last.size == 1) last else incoming
            if (points.isEmpty()) return null
            last = incoming
            active = false
            return Plan(Kind.UP, 0, points, base)
        }

        if (lifted == null) return null
        val points = incoming + lifted
        last = incoming
        return Plan(Kind.POINTER_UP, points.lastIndex, points, base)
    }

    /** 没有在跑的手势就起一条 —— 用于 `move` 先于 `down` 到达（PC 重连后可能如此）。 */
    private fun startIfIdle(now: Long) {
        if (!active) {
            downTime = now
            active = true
        }
    }

    /** 手势内共用按下时刻；没有活跃手势时用当前时刻（事件自成一条手势）。 */
    private fun effectiveDownTime(now: Long): Long = if (active) downTime else now
}
