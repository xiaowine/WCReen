package com.xiaowine.wcreen.agent.proto

import com.google.gson.JsonObject
import com.google.gson.annotations.SerializedName

/*
 * 协议控制消息的 DTO。
 *
 * 三条形态纪律（docs/spec/agent/code.md §5）：
 *  1. 字段名与协议字段逐字一致（`-dontobfuscate` 保证运行期名字不变）；
 *  2. 每个字段都有默认值 —— 主构造器参数全有默认值时 Kotlin 会生成无参构造，
 *     Gson 于是走构造器路径，默认值生效、非空类型安全。否则 Gson 用 Unsafe 绕过构造器，
 *     非空字段在运行期就是 null；
 *  3. 字段用 `var` —— Gson 靠反射赋值。
 *
 * 全部 DTO 必须在 `keepRules/rules.keep` 里 keep：R8 看不见反射访问。
 */

/** 握手（PC→agent，`0x10`）：只有一个协议版本号。 */
internal class HelloMsg(var version: Int = 0)

/** 握手应答（agent→PC，`0x11`）。 */
internal class HelloAckMsg(
    var version: Int = 0,
    var state: String = STATE_READY,
) {
    /** `state` 的取值。 */
    companion object {

        /** 就绪 —— 可以接受「申请屏幕」。 */
        const val STATE_READY: String = "ready"

        /** 无硬编编码器可用 —— 后续「申请屏幕」一律被拒。 */
        const val STATE_NO_HW_ENCODER: String = "noHwEncoder"
    }
}

/** 心跳（双向，`0x30`）：负载是空 JSON 对象，不承载任何协议内容。 */
internal class HeartbeatMsg

/**
 * 指令信封（PC→agent，`0x20`）：具体哪条看 [cmd]。
 *
 * 形状是**信封 + 载荷结构体**：`cmd` / `requestId` 是信封，指令自己的字段全部落在
 * [payload] 里，按 [cmd] 解成对应的 `*Args`（`ApplyScreenArgs` / `InputArgs` …）。
 *
 * 无字段指令（`stopAgent` / `endSession` / `requestSyncFrame`）**省略** `payload` ⇒ [payload] 为 null。
 *
 * @property cmd 指令名，取值见 [Cmd]
 * @property requestId PC 生成的关联号，回报时原样带回
 * @property payload 指令载荷；无载荷指令为 null
 */
internal class CommandMsg(
    var cmd: String = "",
    var requestId: Int = 0,
    var payload: JsonObject? = null,
)

/**
 * 指令回报（agent→PC，`0x21`）。
 *
 * `requestId` 是 PC 生成、必须原样带回的关联号；心跳与主动上报不带它。
 *
 * ⚠️ `applyScreen` **允许部分成功** ⇒ 失败时 `ok=false` + [error] + **顶层** [applied]
 * （已生效的那部分），不丢信息；成功时才用 [result]。
 */
internal class CommandAckMsg(
    var requestId: Int = 0,
    var ok: Boolean = false,
    var result: ResultMsg? = null,
    var applied: AppliedMsg? = null,
    var error: ErrorMsg? = null,
)

/**
 * 指令回报里的 `result` 段 —— **只在成功时出现**。
 *
 * @property streamId agent 分配的屏标识
 * @property applied 每个参数的实际生效值
 */
internal class ResultMsg(
    var streamId: Int = 0,
    var applied: AppliedMsg? = null,
)

/**
 * 主动上报（agent→PC，`0x22`）：agent 单方面改变的事实。
 *
 * @property event 事件名（`screenReleased` / `subsystemUnavailable` / `fatal`）
 * @property reason 原因，诊断用
 * @property subsystem 出问题的子系统名 —— **只有 `subsystemUnavailable` 带它**
 */
internal class EventMsg(
    var event: String = "",
    var reason: String? = null,
    var subsystem: String? = null,
)

/**
 * 错误对象（`transport-messages.md` §4.1）。
 *
 * [level] 在 JSON 里的字段名是 `class` —— Kotlin 关键字不能当属性名，故用 [SerializedName] 改名。
 */
internal class ErrorMsg(
    @SerializedName("class") var level: String = "",
    var code: String = "",
    var message: String = "",
)

/** 指令名 —— 与 `transport-messages.md` §2.3 的 `cmd` 取值逐字对应。 */
internal object Cmd {

    /** 申请屏幕：建屏 / 重连认领 / 参数变更。 */
    const val APPLY_SCREEN: String = "applyScreen"

    /** 结束会话：拆屏但继续监听。 */
    const val END_SESSION: String = "endSession"

    /** 停止 agent：全局收尾并退出进程。 */
    const val STOP_AGENT: String = "stopAgent"

    /** 枚举可启动应用。 */
    const val ENUMERATE_APPS: String = "enumerateApps"

    /** 打开目标应用。 */
    const val LAUNCH_APP: String = "launchApp"

    /** 请求同步帧（主动刷新 / 丢包重同步）。 */
    const val REQUEST_SYNC_FRAME: String = "requestSyncFrame"

    /** 设定会话策略。 */
    const val SET_POLICY: String = "setPolicy"

    /** 输入事件。 */
    const val INPUT: String = "input"
}
