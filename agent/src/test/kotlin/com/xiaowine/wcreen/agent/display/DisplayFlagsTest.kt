package com.xiaowine.wcreen.agent.display

import com.xiaowine.wcreen.agent.wrappers.VirtualDisplayFlags
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * [DisplayFlags] 的位组合测试。
 *
 * 常量值在测试里用**假值**（互不相同的 2 的幂）—— 真实值来自运行期反射，
 * 单测只需要验证**组合规则**本身。规则出处：`docs/spec/agent/display-flags.md`。
 */
class DisplayFlagsTest {

    /** 建屏位（虚拟屏命名空间）。 */
    private val build = VirtualDisplayFlags.BuildFlags(
        public = 1 shl 0,
        ownContentOnly = 1 shl 1,
        supportsTouch = 1 shl 2,
        destroyContentOnRemoval = 1 shl 3,
        shouldShowSystemDecorations = 1 shl 4,
        trusted = 1 shl 5,
        ownDisplayGroup = 1 shl 6,
        alwaysUnlocked = 1 shl 7,
        touchFeedbackDisabled = 1 shl 8,
        ownFocus = 1 shl 9,
    )

    /** 屏回读位（另一套命名空间，故意与上面错开，用来验证不会混用）。 */
    private val readback = VirtualDisplayFlags.ReadbackFlags(
        shouldShowSystemDecorations = 1 shl 16,
        trusted = 1 shl 17,
        ownDisplayGroup = 1 shl 18,
        alwaysUnlocked = 1 shl 19,
        touchFeedbackDisabled = 1 shl 20,
        ownFocus = 1 shl 21,
        presentation = 1 shl 22,
        privateFlag = 1 shl 23,
    )

    private val flags = VirtualDisplayFlags(build, readback)

    @Test
    fun `请求解锁态时含七个必须位且不派生显示组`() {
        val composed = DisplayFlags.compose(flags, showSystemDecorations = false, alwaysUnlocked = true)

        // 七个必须位一个都不能少。
        for (bit in listOf(
            build.trusted,
            build.public,
            build.ownContentOnly,
            build.supportsTouch,
            build.ownFocus,
            build.touchFeedbackDisabled,
            build.destroyContentOnRemoval,
        )) {
            assertTrue("必须位 0x${bit.toString(16)} 缺失", composed and bit != 0)
        }
        // 请求解锁态 ⇒ 置该位。
        assertNotEquals(0, composed and build.alwaysUnlocked)
        // **不**派生独立显示组：屏挂在虚拟设备上，平台自己把设备屏放进设备组
        // （自己再请求只会把屏从设备组里拿出来）。
        assertEquals(0, composed and build.ownDisplayGroup)
        // 缺省不显示系统装饰。
        assertEquals(0, composed and build.shouldShowSystemDecorations)
    }

    @Test
    fun `关掉解锁态两位都不出现`() {
        val composed = DisplayFlags.compose(flags, showSystemDecorations = false, alwaysUnlocked = false)
        assertEquals(0, composed and build.alwaysUnlocked)
        assertEquals(0, composed and build.ownDisplayGroup)
    }

    @Test
    fun `两个可配位独立生效`() {
        val composed = DisplayFlags.compose(flags, showSystemDecorations = true, alwaysUnlocked = false)
        assertNotEquals(0, composed and build.shouldShowSystemDecorations)
        assertEquals(0, composed and build.alwaysUnlocked)
    }

    @Test
    fun `回读期望值与请求一致`() {
        for (decorations in listOf(false, true)) {
            for (unlocked in listOf(false, true)) {
                val requested = DisplayFlags.compose(flags, decorations, unlocked)
                val expected = DisplayFlags.expectedReadback(flags, decorations, unlocked)
                val mask = DisplayFlags.readbackMask(flags)

                // 期望值必须落在掩码内 —— 否则"回读校验"永远不成立。
                assertEquals(expected, expected and mask)
                // 必须置位的位。
                assertNotEquals(0, expected and readback.trusted)
                assertNotEquals(0, expected and readback.touchFeedbackDisabled)
                assertNotEquals(0, expected and readback.ownFocus)
                // 两个"必须未置位"的位。
                assertEquals(0, expected and readback.presentation)
                assertEquals(0, expected and readback.privateFlag)
                // 独立显示组**必须未置位**（掩码含该位 ⇒ 平台一旦置上就是回读不符）。
                assertEquals(0, expected and readback.ownDisplayGroup)
                // 解锁位跟着请求走。
                assertEquals(unlocked, expected and readback.alwaysUnlocked != 0)
                // 请求与期望用的是两套命名空间，位不该相同（防混用）。
                assertNotEquals(requested and 0xFFFF, expected and 0xFFFF)
            }
        }
    }

    @Test
    fun `掩码覆盖八个可观测位`() {
        val mask = DisplayFlags.readbackMask(flags)
        val expected = readback.trusted or readback.ownDisplayGroup or readback.alwaysUnlocked or
            readback.touchFeedbackDisabled or readback.ownFocus or readback.shouldShowSystemDecorations or
            readback.presentation or readback.privateFlag
        assertEquals(expected, mask)
    }
}
