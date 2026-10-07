package com.xiaowine.wcreen.agent.proto

/*
 * 「输入事件」指令（`0x20` 的 `input`）的 DTO。
 *
 * 形态纪律同 `Control.kt`。字段语义出处：`docs/spec/agent/input.md` §2 / §5 / §6。
 */

/**
 * 输入事件的**载荷**（`0x20` 的 `input`，落在信封的 `payload` 里）。
 *
 * 它不再是「消息」—— 不含 `cmd` / `requestId`（那两个在信封上）。
 *
 * `action` 决定读哪个载荷字段：触摸类读 [pointers]，滚轮读 [scroll]，按键读 [key]。
 *
 * @property action 事件种类，取值见 [InputAction]
 * @property pointers 触点快照 —— **变更后**的当前触点集合，屏像素整数
 * @property scroll 滚轮增量位移；`action = scroll` 时必给
 * @property key 按键；`action = keyDown` / `keyUp` 时必给
 */
internal class InputArgs(
    var action: String = "",
    var pointers: List<PointerMsg> = emptyList(),
    var scroll: ScrollMsg? = null,
    var key: KeyMsg? = null,
)

/**
 * 一个触点。
 *
 * @property id 触点标识（PC 分配、手势内稳定）—— agent 由它推导 Android 的 `pointerIndex`
 * @property x 屏像素 x
 * @property y 屏像素 y
 */
internal class PointerMsg(
    var id: Int = 0,
    var x: Int = 0,
    var y: Int = 0,
)

/**
 * 滚轮位移 —— **增量**语义（所以队列合并时必须累加，绝不覆盖，见 `input.md` §5）。
 *
 * [x] / [y] 是滚轮发生的位置：Android 的 `ACTION_SCROLL` 要用坐标做命中判定。
 * [dx] / [dy] 的正负号与 Android 的 `AXIS_HSCROLL` / `AXIS_VSCROLL` 一致，agent 不做转换。
 *
 * @property dx 横向增量
 * @property dy 纵向增量
 * @property x 位置 x（屏像素）
 * @property y 位置 y（屏像素）
 */
internal class ScrollMsg(
    var dx: Int = 0,
    var dy: Int = 0,
    var x: Int = 0,
    var y: Int = 0,
)

/**
 * 按键 —— 直接给 Android 的 `keyCode` 与 `metaState`（agent 侧零映射，见 `input.md` §6）。
 *
 * @property code Android keyCode
 * @property meta metaState（Shift / Ctrl / Alt 等修饰键）
 */
internal class KeyMsg(
    var code: Int = 0,
    var meta: Int = 0,
)

/** `action` 的取值 —— 与 `docs/spec/agent/input.md` §2 逐字对应。 */
internal object InputAction {

    /** 触点按下（首个触点）。 */
    const val DOWN: String = "down"

    /** 触点移动。 */
    const val MOVE: String = "move"

    /** 触点抬起。 */
    const val UP: String = "up"

    /** 手势取消。 */
    const val CANCEL: String = "cancel"

    /** 滚轮。 */
    const val SCROLL: String = "scroll"

    /** 按键按下。 */
    const val KEY_DOWN: String = "keyDown"

    /** 按键抬起。 */
    const val KEY_UP: String = "keyUp"
}
