package com.xiaowine.wcreen.agent.wrappers

import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome

/**
 * 虚拟屏 flag 的常量值。
 *
 * **两套命名空间必须分清**（数值不同，"可信"在虚拟屏位是 1024、在屏回读位是 128）：
 * - [build] —— 建屏时传给 `createVirtualDisplay` 的位；
 * - [readback] —— `Display.getFlags()` 回读出来的位，**只有一部分**虚拟屏位在这里有对应值。
 *
 * @property build 建屏位
 * @property readback 屏回读位
 */
internal class VirtualDisplayFlags(val build: BuildFlags, val readback: ReadbackFlags) {

    /**
     * 建屏位（`android.hardware.display.DisplayManager.VIRTUAL_DISPLAY_FLAG_*`）。
     *
     * @property public 公开屏
     * @property ownContentOnly 无窗口时不自镜像
     * @property supportsTouch 支持触摸
     * @property destroyContentOnRemoval 移除时销毁内容
     * @property shouldShowSystemDecorations 显示系统装饰
     * @property trusted 可信
     * @property ownDisplayGroup 独立显示组
     * @property alwaysUnlocked 不显示 keyguard
     * @property touchFeedbackDisabled 关触摸反馈
     * @property ownFocus 维持自身焦点
     */
    class BuildFlags(
        val public: Int,
        val ownContentOnly: Int,
        val supportsTouch: Int,
        val destroyContentOnRemoval: Int,
        val shouldShowSystemDecorations: Int,
        val trusted: Int,
        val ownDisplayGroup: Int,
        val alwaysUnlocked: Int,
        val touchFeedbackDisabled: Int,
        val ownFocus: Int,
    )

    /**
     * 屏回读位（`android.view.Display.FLAG_*`）—— 只登记**可观测**的那些。
     *
     * 虚拟屏位里的 `PUBLIC` / `OWN_CONTENT_ONLY` / `SUPPORTS_TOUCH` /
     * `DESTROY_CONTENT_ON_REMOVAL` 在这个命名空间里**没有对应位**，无法回读校验。
     *
     * @property shouldShowSystemDecorations 显示系统装饰
     * @property trusted 可信
     * @property ownDisplayGroup 独立显示组
     * @property alwaysUnlocked 不显示 keyguard
     * @property touchFeedbackDisabled 关触摸反馈
     * @property ownFocus 维持自身焦点
     * @property presentation 投影屏（**必须未置位**）
     * @property privateFlag 私有屏（**必须未置位**）
     */
    class ReadbackFlags(
        val shouldShowSystemDecorations: Int,
        val trusted: Int,
        val ownDisplayGroup: Int,
        val alwaysUnlocked: Int,
        val touchFeedbackDisabled: Int,
        val ownFocus: Int,
        val presentation: Int,
        val privateFlag: Int,
    )

    /**
     * 解析全部常量。
     *
     * @return 成功，或**启动级**失败并**列出缺哪几条**（把"用到才发现"前移成"启动就发现"）
     */
    companion object {

        /** 建屏位所在的类。 */
        private const val DISPLAY_MANAGER = "android.hardware.display.DisplayManager"

        /** 屏回读位所在的类。 */
        private const val DISPLAY = "android.view.Display"

        /** 建屏位字段名（顺序与 [BuildFlags] 的参数一致）。 */
        private val BUILD_FIELDS = listOf(
            "VIRTUAL_DISPLAY_FLAG_PUBLIC",
            "VIRTUAL_DISPLAY_FLAG_OWN_CONTENT_ONLY",
            "VIRTUAL_DISPLAY_FLAG_SUPPORTS_TOUCH",
            "VIRTUAL_DISPLAY_FLAG_DESTROY_CONTENT_ON_REMOVAL",
            "VIRTUAL_DISPLAY_FLAG_SHOULD_SHOW_SYSTEM_DECORATIONS",
            "VIRTUAL_DISPLAY_FLAG_TRUSTED",
            "VIRTUAL_DISPLAY_FLAG_OWN_DISPLAY_GROUP",
            "VIRTUAL_DISPLAY_FLAG_ALWAYS_UNLOCKED",
            "VIRTUAL_DISPLAY_FLAG_TOUCH_FEEDBACK_DISABLED",
            "VIRTUAL_DISPLAY_FLAG_OWN_FOCUS",
        )

        /** 屏回读位字段名（顺序与 [ReadbackFlags] 的参数一致）。 */
        private val READBACK_FIELDS = listOf(
            "FLAG_SHOULD_SHOW_SYSTEM_DECORATIONS",
            "FLAG_TRUSTED",
            "FLAG_OWN_DISPLAY_GROUP",
            "FLAG_ALWAYS_UNLOCKED",
            "FLAG_TOUCH_FEEDBACK_DISABLED",
            "FLAG_OWN_FOCUS",
            "FLAG_PRESENTATION",
            "FLAG_PRIVATE",
        )

        /** 解析结果；null 表示尚未解析或解析失败。 */
        private var resolved: VirtualDisplayFlags? = null

        /**
         * 解析全部常量。
         *
         * @return 成功，或启动级失败
         */
        fun resolve(): Outcome<Unit> {
            val missing = ArrayList<String>()

            /** 取一个 static final int 常量。 */
            fun constant(owner: String, name: String): Int? = try {
                Class.forName(owner).getDeclaredField(name).also { it.isAccessible = true }.getInt(null)
            } catch (e: Exception) {
                missing += "$owner#$name"
                null
            }

            val buildValues = BUILD_FIELDS.map { constant(DISPLAY_MANAGER, it) }
            val readbackValues = READBACK_FIELDS.map { constant(DISPLAY, it) }

            if (missing.isNotEmpty()) {
                return Outcome.Err(
                    Fail(
                        FailLevel.FATAL,
                        FailCode.WRAPPER_MISSING,
                        "缺少 ${missing.size} 个虚拟屏 flag 常量：${missing.joinToString(", ")}",
                    ),
                )
            }

            resolved = VirtualDisplayFlags(
                BuildFlags(
                    public = buildValues[0]!!,
                    ownContentOnly = buildValues[1]!!,
                    supportsTouch = buildValues[2]!!,
                    destroyContentOnRemoval = buildValues[3]!!,
                    shouldShowSystemDecorations = buildValues[4]!!,
                    trusted = buildValues[5]!!,
                    ownDisplayGroup = buildValues[6]!!,
                    alwaysUnlocked = buildValues[7]!!,
                    touchFeedbackDisabled = buildValues[8]!!,
                    ownFocus = buildValues[9]!!,
                ),
                ReadbackFlags(
                    shouldShowSystemDecorations = readbackValues[0]!!,
                    trusted = readbackValues[1]!!,
                    ownDisplayGroup = readbackValues[2]!!,
                    alwaysUnlocked = readbackValues[3]!!,
                    touchFeedbackDisabled = readbackValues[4]!!,
                    ownFocus = readbackValues[5]!!,
                    presentation = readbackValues[6]!!,
                    privateFlag = readbackValues[7]!!,
                ),
            )
            return Outcome.Ok(Unit)
        }

        /**
         * 取已解析的常量。
         *
         * @return 常量集合
         * @throws IllegalStateException 尚未 [resolve] —— 属编程错误
         */
        fun get(): VirtualDisplayFlags = requireNotNull(resolved) { "虚拟屏 flag 常量尚未解析" }
    }
}
