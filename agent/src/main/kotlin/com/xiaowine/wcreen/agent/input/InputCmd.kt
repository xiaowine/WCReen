package com.xiaowine.wcreen.agent.input

import com.xiaowine.wcreen.agent.proto.InputArgs

/**
 * 投给 `input` 线程的一条消息。
 *
 * 线程之间只能靠**队列**通信（`code.md` §6）⇒ 连"目标屏是哪一块"也是**投递**过去的，
 * 而不是共享变量：屏的 owner 是 `link` 线程，`input` 不该去读它的状态。
 */
internal sealed class InputCmd {

    /**
     * 注入一条输入事件。
     *
     * @property msg 已解码的输入载荷
     * @property requestId 信封里的关联号；失败回报时原样带回（成功静默）。
     *   滚轮合并时它跟着取最新 —— 合并后的那一条代表最新请求。
     */
    class Event(val msg: InputArgs, var requestId: Int) : InputCmd()

    /**
     * 目标屏变了 —— 由 `link` 在建屏 / 拆屏时投递。
     *
     * @property displayId 新的目标显示 id；`<= 0` 表示当前无屏
     */
    class ScreenChanged(val displayId: Int) : InputCmd()
}
