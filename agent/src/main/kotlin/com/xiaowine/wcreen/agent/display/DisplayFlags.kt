package com.xiaowine.wcreen.agent.display

import com.xiaowine.wcreen.agent.wrappers.VirtualDisplayFlags

/**
 * 虚拟屏 flag 的组合与校验规则。
 *
 * **纯函数**（常量值由调用方传入）⇒ 不依赖任何 Android 类型 ⇒ 能进 JVM 单测
 * （`docs/spec/agent/code.md` §8）。规则出处：`docs/spec/agent/display-flags.md`。
 */
internal object DisplayFlags {

    /**
     * 组装建屏 flags。
     *
     * - **必须位 7 个**：`TRUSTED`（否则第三方 App 上不去）/ `PUBLIC`（uid 2000 建的屏要公开）/
     *   `OWN_CONTENT_ONLY`（否则无窗口时会镜像本机屏）/ `SUPPORTS_TOUCH`（产品决定）/
     *   `OWN_FOCUS`（无害，官方推荐组合）/ `TOUCH_FEEDBACK_DISABLED` / `DESTROY_CONTENT_ON_REMOVAL`
     *   （关掉会在**拆屏与崩溃两条路径**上把内容甩到本机屏）；
     * - **PC 可配 1 个**：`SHOULD_SHOW_SYSTEM_DECORATIONS`（缺省关）；
     * - **解锁位**（`ALWAYS_UNLOCKED`）：**缺省开**，语义 = "主屏锁屏 / 息屏时本屏照常可用"
     *   （2026-10-07 双档像素取证）；
     * - **不请求 `OWN_DISPLAY_GROUP`**：屏挂在**虚拟设备**上，平台会自动把设备屏放进**设备自己的**
     *   显示组（AOSP `DisplayManagerService`），解锁位因此被接受；自己再请求该位只会平白把屏从
     *   设备组里拿出来。**注意**：解锁位只对**非默认组**的屏有效，默认组里的屏会被平台**直接丢弃**
     *   该位（同上源码）—— 这就是"建屏必须走虚拟设备"的原因。
     *   依据：`docs/plans/2026-10-07-vdm-mechanism-corrected.md` §2/§3。
     *
     * @param flag 已解析的常量值
     * @param showSystemDecorations 是否显示系统装饰（PC 可配，缺省关）
     * @param alwaysUnlocked 是否保持解锁态（缺省开）
     * @return 建屏用的 flags 位组合
     */
    fun compose(flag: VirtualDisplayFlags, showSystemDecorations: Boolean, alwaysUnlocked: Boolean): Int {
        val b = flag.build
        var flags = b.trusted or b.public or b.ownContentOnly or b.supportsTouch or
            b.ownFocus or b.touchFeedbackDisabled or b.destroyContentOnRemoval
        if (showSystemDecorations) flags = flags or b.shouldShowSystemDecorations
        if (alwaysUnlocked) flags = flags or b.alwaysUnlocked
        return flags
    }

    /**
     * 回读校验的掩码 —— 只覆盖**可观测**的位。
     *
     * 虚拟屏位里的 `PUBLIC` / `OWN_CONTENT_ONLY` / `SUPPORTS_TOUCH` /
     * `DESTROY_CONTENT_ON_REMOVAL` 在屏回读命名空间里没有对应位，不在掩码内。
     *
     * @param flag 已解析的常量值
     * @return 掩码
     */
    fun readbackMask(flag: VirtualDisplayFlags): Int {
        val r = flag.readback
        return r.trusted or r.ownDisplayGroup or r.alwaysUnlocked or r.touchFeedbackDisabled or
            r.ownFocus or r.shouldShowSystemDecorations or r.presentation or r.privateFlag
    }

    /**
     * 期望的回读值（同一套规则，换到屏回读命名空间）。
     *
     * `PRESENTATION` 与 `PRIVATE` **必须未置位** —— 它们是"不镜像、公开"两条意图的反面。
     *
     * @param flag 已解析的常量值
     * @param showSystemDecorations 是否显示系统装饰
     * @param alwaysUnlocked 是否保持解锁态
     * @return 期望的回读位
     */
    fun expectedReadback(
        flag: VirtualDisplayFlags,
        showSystemDecorations: Boolean,
        alwaysUnlocked: Boolean,
    ): Int {
        val r = flag.readback
        var expected = r.trusted or r.touchFeedbackDisabled or r.ownFocus
        if (showSystemDecorations) expected = expected or r.shouldShowSystemDecorations
        // 解锁位照请求走；`OWN_DISPLAY_GROUP` **必须未置位**（新路径不请求它，平台也不会给
        // 虚拟设备的屏加它 —— 掩码里含该位，一旦出现就是回读不符 ⇒ 致命）。
        if (alwaysUnlocked) expected = expected or r.alwaysUnlocked
        return expected
    }
}
