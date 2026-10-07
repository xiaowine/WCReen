package com.xiaowine.wcreen.agent.wrappers

import android.hardware.display.VirtualDisplayConfig
import java.lang.reflect.Method

/**
 * 建屏配置上的隐藏开关：**忽略活动的尺寸限制**
 * （`android.hardware.display.VirtualDisplayConfig.Builder#setIgnoreActivitySizeRestrictions(boolean)`）。
 *
 * # 为什么需要它
 *
 * 屏挂在**虚拟设备**上（`wrappers/VirtualDevice.kt`）⇒ 该屏 **`mFixedToUserRotation=true`**（实测）
 * ⇒ **永不为应用的固定方向请求旋转** ⇒ **不可缩放 + 固定方向**的应用走 A15/14 的
 * **fixed-orientation letterbox**（活动级 letterbox）被裁（2026-10-07 实测：1920×1800 上窗口只有
 * `[223,0][1698,1800]`）。
 * 这个隐藏开关正是"忽略应用的固定方向 / 宽高比 / 可调整性"的官方入口（javadoc 原文：
 * *ignores fixed orientation, aspect ratio and resizability of apps*；**要求屏带 `TRUSTED`**）。
 * 加上它之后同一块屏实测 `[0,0][1920,1800]`、`letterBoxed=false` —— **任意尺寸铺满**。
 * 依据：`docs/spec/agent/display-letterbox.md`（机制与双端实测）。
 *
 * # 在反射清单里的位置
 *
 * 这是 `code-hiddenapi.md` §3 登记的一处运行期反射（**只反射 1 个方法**：
 * `Builder.setIgnoreActivitySizeRestrictions(boolean)` —— `Builder` 本身是公开类，装配走公开 API）。
 *
 * # 版本门槛：**Android 16（SDK 36）起才有**
 *
 * - `api-versions.xml`（公开 API 清单）里**没有**它（隐藏 API）；本机 Android 16 的 `framework.jar`
 *   里**有**它 ⇒ 存在性已实测。
 * - Android 15 的 `framework.jar` 逐 dex 扫字符串 **0 命中** ⇒ 不存在（实测）。
 * - 同类工具 AndroMeld 把它门控在 `SDK_INT >= 36` 并 `catch (LinkageError | …)` —— 旁证。
 * - 因此本对象**必须可降级**：Android 14/15（agent 的 minSdk = 34）上 `resolve()` 返回 `false`，
 *   建屏照常，只是回到平台默认行为（这类 App 可能被裁，见 `docs/spec/agent/display-letterbox.md` §6）。
 *
 * # 失败形态：**降级，不是 fatal**
 *
 * 该开关是**增强**：拿不到它就回到平台默认行为（这类 App 可能被裁，
 * 见 `docs/spec/agent/display-letterbox.md` §6），
 * 建屏本身照常可用 ⇒ 解析失败只**告警一次**，不阻断启动，也不影响回读校验。
 */
internal object ActivitySizeRestrictions {

    /** 已解析的隐藏方法；`null` = 本机没有这个 API（降级态）。 */
    private var method: Method? = null

    /** 是否已尝试解析（解析失败也只尝试一次，避免每次建屏都白试）。 */
    private var attempted = false

    /**
     * 解析隐藏方法（启动期调用一次）。
     *
     * @return `true` = 可用；`false` = 本机没有 ⇒ 调用方应记一条告警并按降级态继续
     */
    fun resolve(): Boolean {
        if (attempted) return method != null
        attempted = true
        method = try {
            VirtualDisplayConfig.Builder::class.java
                .getMethod("setIgnoreActivitySizeRestrictions", Boolean::class.javaPrimitiveType)
                .also { it.isAccessible = true }
        } catch (e: Exception) {
            null
        }
        return method != null
    }

    /**
     * 把"忽略尺寸限制"应用到正在装配的配置上。
     *
     * @param builder 建屏配置的 Builder（公开 API 造的）
     * @return `true` = 已应用；`false` = 降级（方法不可用或调用失败），调用方继续建屏即可
     */
    fun apply(builder: VirtualDisplayConfig.Builder): Boolean {
        val m = method ?: return false
        return try {
            m.invoke(builder, true)
            true
        } catch (e: Exception) {
            false
        }
    }
}
