package com.xiaowine.wcreen.agent.wrappers

import android.content.Context
import android.hardware.input.InputManager
import android.view.InputEvent
import com.xiaowine.wcreen.agent.proto.ErrorCode
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome
import java.lang.reflect.InvocationTargetException
import java.lang.reflect.Method

/**
 * 隐藏 API 域：**输入注入**（`code-hiddenapi.md` §3 第 5 处）。
 *
 * ## 取法（2026-10-07 实测定稿，实体机 Android 16 / SDK 36）
 *
 * | 环节 | 手段 |
 * |---|---|
 * | 取 `InputManager` | **公开**的 `getSystemService(INPUT_SERVICE)` —— **0 反射** |
 * | 注入 | 反射 `InputManager.injectInputEvent(InputEvent, int)` |
 * | 定向到虚拟屏 | 反射 `InputEvent.setDisplayId(int)` —— 按键事件没有公开的 displayId 入口 |
 * | 注入模式 | 反射常量 `INJECT_INPUT_EVENT_MODE_ASYNC` —— **绝不硬编码** |
 *
 * 为什么 `getSystemService` 这次能用而 `DisplayManager` 不能：那处卡的是**建屏时的归属校验**
 * （拿 Context 的包名与调用 uid 比对，真系统 Context 报 `android`）；注入这一路看的是
 * **调用进程的 uid**（`INJECT_EVENTS` 权限），与 Context 的包名无关。
 * 交叉验证：scrcpy 的 `wrappers/InputManager.java` 走的就是同一条路（它同样反射
 * `injectInputEvent` 与 `setDisplayId`，并用伪装 Context 取 `InputManager`）。
 *
 * ⚠️ **模式常量必须反射而非硬编码**：本机实测
 * `ASYNC = 0 / WAIT_FOR_RESULT = 1 / WAIT_FOR_FINISH = 2`，
 * 而 `input.md` §4 原表把 `WAIT_FOR_FINISH` 记成 `1` —— 硬编码就会写错。
 * `WAIT_FOR_FINISH` 实测会让一次注入等满 10 秒（`input.md` §4），本实现只用 `ASYNC`。
 *
 * 按反射纪律：三个成员只在本文件出现，启动时解析一次并缓存，对外只暴露 `InputEvent`。
 */
internal object InputInjector {

    /** 注入器所在类的全名（该类在 SDK 里是**公开**类，故只用于报错信息与字段名归属）。 */
    private const val INPUT_MANAGER = "android.hardware.input.InputManager"

    /** 注入方法名。 */
    private const val INJECT_METHOD = "injectInputEvent"

    /** 设目标屏的方法名（在 `android.view.InputEvent` 上）。 */
    private const val SET_DISPLAY_ID = "setDisplayId"

    /** 异步注入模式的常量名。 */
    private const val MODE_ASYNC = "INJECT_INPUT_EVENT_MODE_ASYNC"

    /**
     * 解析出来的成员集合。
     *
     * 只有本文件看得见它 —— 反射类型（`Method`）不外泄。
     *
     * @property inject `injectInputEvent(InputEvent, int)`
     * @property setDisplayId `InputEvent.setDisplayId(int)`
     * @property asyncMode 异步注入模式的**实测值**
     */
    private class Members(
        val inject: Method,
        val setDisplayId: Method,
        val asyncMode: Int,
    )

    /** 解析结果；null 表示尚未解析或解析失败。 */
    private var members: Members? = null

    /**
     * 解析全部成员。
     *
     * @return 成功，或**启动级**失败并**列出缺哪几条**（把"用到才发现"前移成"启动就发现"）
     */
    fun resolve(): Outcome<Unit> {
        val missing = ArrayList<String>()
        val managerClass = InputManager::class.java
        val intType = Int::class.javaPrimitiveType

        val inject = try {
            managerClass.getMethod(INJECT_METHOD, InputEvent::class.java, intType)
        } catch (e: Exception) {
            missing += "$INPUT_MANAGER#$INJECT_METHOD(InputEvent, int)"
            null
        }
        val setDisplayId = try {
            InputEvent::class.java.getMethod(SET_DISPLAY_ID, intType)
        } catch (e: Exception) {
            missing += "android.view.InputEvent#$SET_DISPLAY_ID(int)"
            null
        }
        val asyncMode = try {
            managerClass.getField(MODE_ASYNC).getInt(null)
        } catch (e: Exception) {
            missing += "$INPUT_MANAGER#$MODE_ASYNC"
            null
        }

        if (missing.isNotEmpty()) {
            return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.WRAPPER_MISSING,
                    "缺少 ${missing.size} 个输入注入成员：${missing.joinToString(", ")}",
                ),
            )
        }
        members = Members(inject!!, setDisplayId!!, asyncMode!!)
        return Outcome.Ok(Unit)
    }

    /**
     * 造一个注入器。
     *
     * @param context **伪装过包名**的 Context（`display/FakeContext.kt`）——
     *   与 scrcpy 一致；注入本身与包名无关，但保持全进程一个来源
     * @return 注入器，或启动级失败
     */
    fun create(context: Context): Outcome<Injector> {
        if (members == null) {
            return Outcome.Err(
                Fail(FailLevel.FATAL, FailCode.WRAPPER_MISSING, "输入注入成员尚未解析"),
            )
        }
        val manager = context.getSystemService(Context.INPUT_SERVICE) as? InputManager
            ?: return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.WRAPPER_MISSING,
                    "getSystemService('${Context.INPUT_SERVICE}') 没有返回 InputManager",
                ),
            )
        return Outcome.Ok(Injector(manager))
    }

    /**
     * 把一个事件注到指定屏。
     *
     * @param manager `InputManager` 实例
     * @param event 待注入事件
     * @param displayId 目标显示 id；**必须 > 0**（调用方保证，见 `runtime/InputLoop.kt`）
     * @return 成功，或失败：**策略禁止是子系统级**（能力缺失，重试无意义），
     *   其余（返回 false / 抛异常）是**可恢复级**
     */
    private fun injectVia(manager: InputManager, event: InputEvent, displayId: Int): Outcome<Unit> {
        val m = members ?: return Outcome.Err(
            Fail(FailLevel.FATAL, FailCode.WRAPPER_MISSING, "输入注入成员尚未解析"),
        )

        // 目标屏必须显式设：事件不带 displayId 会落到主屏 —— 而主屏**绝不能被碰**
        // （`input.md` §1 前提 3：注入期间本机屏 top 完全不变）。
        try {
            m.setDisplayId.invoke(event, displayId)
        } catch (e: Exception) {
            return Outcome.Err(
                Fail(
                    FailLevel.RECOVERABLE,
                    ErrorCode.INPUT_FAILED,
                    "设置事件目标屏失败（displayId=$displayId）：${e.message}",
                    e,
                ),
            )
        }

        return try {
            if (m.inject.invoke(manager, event, m.asyncMode) as? Boolean == true) {
                Outcome.Ok(Unit)
            } else {
                Outcome.Err(
                    Fail(FailLevel.RECOVERABLE, ErrorCode.INPUT_FAILED, "注入返回 false"),
                )
            }
        } catch (e: InvocationTargetException) {
            val cause = e.cause
            if (cause is SecurityException) {
                // 能力缺失（最常见：USB 调试（安全设置）没开）⇒ 子系统级，重试无意义。
                Outcome.Err(
                    Fail(
                        FailLevel.SUBSYSTEM,
                        ErrorCode.INPUT_DENIED,
                        "注入被系统策略禁止：${cause.message}",
                        cause,
                    ),
                )
            } else {
                Outcome.Err(
                    Fail(
                        FailLevel.RECOVERABLE,
                        ErrorCode.INPUT_FAILED,
                        "注入抛异常：${cause?.message}",
                        cause ?: e,
                    ),
                )
            }
        } catch (e: Exception) {
            Outcome.Err(
                Fail(FailLevel.RECOVERABLE, ErrorCode.INPUT_FAILED, "注入失败：${e.message}", e),
            )
        }
    }

    /**
     * 一路输入注入器 —— 持有 `InputManager` 实例，隐藏反射细节。
     *
     * @property manager 由伪装 Context 取到的 `InputManager`
     */
    internal class Injector internal constructor(private val manager: InputManager) {

        /**
         * 注一条事件到指定屏（异步模式）。
         *
         * @param event 待注入事件
         * @param displayId 目标显示 id
         * @return 成功，或带级别的失败
         */
        fun inject(event: InputEvent, displayId: Int): Outcome<Unit> =
            injectVia(manager, event, displayId)
    }
}
