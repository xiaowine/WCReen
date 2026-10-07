package com.xiaowine.wcreen.agent.wrappers

import android.os.Looper
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome
import java.lang.reflect.Field

/**
 * 隐藏 API 域：主线程 Looper 的静态字段 `android.os.Looper#sMainLooper`。
 *
 * 为什么躲不掉：公开的 `Looper.prepareMainLooper()` 会 `quitAllowed = false`
 * ⇒ 主线程**退不掉**，收尾的 `finally` 永不执行。只能先用 `Looper.prepare()` 建一个
 * 可退出的 Looper，再把这个字段补上（不补则 `Looper.getMainLooper()` 为 null，
 * 部分框架路径会炸）。
 *
 * 按反射纪律：**成员名只在本文件出现**，启动时解析一次并缓存，对外只暴露普通 Kotlin 类型。
 */
internal object MainLooper {

    /** 字段名 —— 只在这一处出现。 */
    private const val FIELD_NAME = "sMainLooper"

    /** 解析结果；null 表示尚未解析或解析失败。 */
    private var cached: Field? = null

    /** 是否已解析成功。 */
    val resolved: Boolean get() = cached != null

    /**
     * 把 [looper] 登记为进程的主 Looper。
     *
     * @param looper 当前线程已经 `prepare()` 出来的 Looper
     * @return 成功，或**启动级**失败（缺成员 / 写入被拒）—— 调用方应据此以退出码 2 结束
     */
    fun install(looper: Looper): Outcome<Unit> {
        val field = cached ?: try {
            Looper::class.java.getDeclaredField(FIELD_NAME).also { it.isAccessible = true }
        } catch (e: Exception) {
            return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.WRAPPER_MISSING,
                    "缺少隐藏成员 android.os.Looper#$FIELD_NAME",
                    e,
                ),
            )
        }

        try {
            field.set(null, looper)
        } catch (e: Exception) {
            return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.WRAPPER_MISSING,
                    "写入 android.os.Looper#$FIELD_NAME 失败",
                    e,
                ),
            )
        }

        cached = field
        return Outcome.Ok(Unit)
    }
}
