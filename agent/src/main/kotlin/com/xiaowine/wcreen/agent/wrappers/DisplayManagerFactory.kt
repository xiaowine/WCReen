package com.xiaowine.wcreen.agent.wrappers

import android.content.Context
import android.hardware.display.DisplayManager
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome
import java.lang.reflect.Constructor

/**
 * 隐藏 API 域：`android.hardware.display.DisplayManager` 的**隐藏构造器**。
 *
 * 为什么躲不掉：公开的 `Context.getSystemService(DISPLAY_SERVICE)` 返回的实例**绑的是真系统
 * Context**（包名 `android`），不是我们的伪装 Context ⇒ 建屏时 system_server 侧报
 * `SecurityException: packageName must match the calling uid`。
 * 唯一出路是拿隐藏构造器把伪装 Context 塞进去。
 *
 * 按反射纪律：构造器只在本文件解析一次并缓存，对外只暴露 `DisplayManager`。
 */
internal object DisplayManagerFactory {

    /** 构造器签名 —— 只在这一处出现。 */
    private var ctor: Constructor<DisplayManager>? = null

    /**
     * 解析隐藏构造器。
     *
     * @return 成功，或**启动级**失败
     */
    fun resolve(): Outcome<Unit> {
        val found = try {
            DisplayManager::class.java.getDeclaredConstructor(Context::class.java).also {
                it.isAccessible = true
            }
        } catch (e: Exception) {
            return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.WRAPPER_MISSING,
                    "缺少隐藏成员 android.hardware.display.DisplayManager(Context)",
                    e,
                ),
            )
        }
        ctor = found
        return Outcome.Ok(Unit)
    }

    /**
     * 用指定的 Context 造一个 `DisplayManager`。
     *
     * @param context 必须是**伪装过包名**的 Context（见 `display/FakeContext.kt`），
     *   否则 system_server 侧判定失败
     * @return 实例，或启动级失败
     */
    fun create(context: Context): Outcome<DisplayManager> {
        val c = ctor ?: return Outcome.Err(
            Fail(FailLevel.FATAL, FailCode.WRAPPER_MISSING, "DisplayManager 构造器尚未解析"),
        )
        return try {
            Outcome.Ok(c.newInstance(context))
        } catch (e: Exception) {
            Outcome.Err(
                Fail(FailLevel.FATAL, FailCode.WRAPPER_MISSING, "构造 DisplayManager 失败", e),
            )
        }
    }
}
