package com.xiaowine.wcreen.agent.display

import android.hardware.display.VirtualDisplay
import android.hardware.display.VirtualDisplayConfig
import android.view.Surface
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Ln
import com.xiaowine.wcreen.agent.runtime.Outcome
import com.xiaowine.wcreen.agent.wrappers.ActivitySizeRestrictions
import com.xiaowine.wcreen.agent.wrappers.VirtualDevice
import com.xiaowine.wcreen.agent.wrappers.VirtualDisplayFlags

/**
 * 一块虚拟屏 —— 只负责"屏"这一半，编码器由 [com.xiaowine.wcreen.agent.encode.VideoEncoder] 管。
 *
 * **owner 纪律**：所有方法只允许在 `link` 线程调用（`docs/spec/agent/runtime-threads.md` §1）。
 *
 * 屏挂在**虚拟设备**上（`wrappers/VirtualDevice.kt`）：这是"锁屏/息屏不接管该屏"与"App 铺满"
 * 两条要求能同时成立的唯一路径（因果链见 `docs/plans/2026-10-07-vdm-mechanism-corrected.md`）。
 *
 * @param ln 日志出口
 * @param device 虚拟设备句柄（进程级共享，一个设备挂多块屏）
 * @param flags 已解析的 flag 常量
 */
internal class Screen(
    private val ln: Ln,
    private val device: VirtualDevice.Handle,
    private val flags: VirtualDisplayFlags,
) {

    /** 当前屏；null 表示"无屏"态。 */
    private var virtualDisplay: VirtualDisplay? = null

    /** 建屏时用的参数 —— 回读校验与"参数是否变了"的比较都要用。 */
    private var requested: Request? = null

    /** 屏是否存在。 */
    val exists: Boolean get() = virtualDisplay != null

    /** 系统分配的显示 id；无屏时为 -1。 */
    val displayId: Int get() = virtualDisplay?.display?.displayId ?: -1

    /**
     * 建屏。
     *
     * @param name 屏名（`WCReen-<n>`，便于 `dumpsys display` 排查与多路区分）
     * @param width 宽（像素）—— **必须等于编码尺寸**（不允许 GPU 缩放）
     * @param height 高（像素）
     * @param dpi 密度
     * @param showSystemDecorations 是否显示系统装饰（PC 可配，缺省关）
     * @param alwaysUnlocked 是否保持解锁态（PC 可配，**缺省关** —— 见 `display-flags.md` §3）
     * @param surface 输出面（编码器的输入面）
     * @return 成功，或失败：**建屏被拒是可恢复级**（资源类，PC 可稍后重试），
     *   **回读不符是致命级**（平台没照做，屏不是我们承诺的东西）
     */
    fun create(
        name: String,
        width: Int,
        height: Int,
        dpi: Int,
        showSystemDecorations: Boolean,
        alwaysUnlocked: Boolean,
        surface: Surface,
    ): Outcome<Unit> {
        val composed = DisplayFlags.compose(flags, showSystemDecorations, alwaysUnlocked)
        val builder = VirtualDisplayConfig.Builder(name, width, height, dpi)
            .setFlags(composed)
            .setSurface(surface)
        // 「忽略活动尺寸限制」（`wrappers/ActivitySizeRestrictions` = 隐藏开关；Android 16 起才有，
        // 拿不到即**降级**）：VDM 建的屏 `mFixedToUserRotation=true`（实测）⇒ **永不为 App 的固定方向
        // 请求旋转** ⇒ 不可缩放 + 固定方向的 App 走 A15/14 的 **fixed-orientation letterbox**（活动级
        // letterbox）被裁。这个开关正是取消这类尺寸限制的入口；**14/15 没有它 ⇒ 这类 App 会被裁**
        // （产品决定不为此另做动作，见 `docs/spec/agent/display-letterbox.md` §6）。
        // 依据：docs/spec/agent/display-letterbox.md（机制）· docs/plans/2026-10-07-vdm-mechanism-corrected.md（建屏）。
        val sizeIgnored = ActivitySizeRestrictions.apply(builder)
        ln.i("建屏配置：sizeRestrictionsIgnored=$sizeIgnored")
        val vd = when (val r = VirtualDevice.createDisplay(device, builder.build())) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> return r
        }

        // 回读校验：平台有没有照做，只有回读才知道（全局规则「不依赖默认值」）。
        val actual = vd.display.flags
        val mask = DisplayFlags.readbackMask(flags)
        val expected = DisplayFlags.expectedReadback(flags, showSystemDecorations, alwaysUnlocked)
        if (actual and mask != expected and mask) {
            vd.release()
            return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.SCREEN_VERIFY,
                    "建屏后回读 flags 不符：期望 0x%x，实际 0x%x（掩码 0x%x）".format(
                        expected and mask, actual and mask, mask,
                    ),
                ),
            )
        }

        virtualDisplay = vd
        requested = Request(name, width, height, dpi, showSystemDecorations, alwaysUnlocked)
        ln.i("建屏成功：$name ${width}x$height@$dpi displayId=${vd.display.displayId} flags=0x%x".format(composed))
        return Outcome.Ok(Unit)
    }

    /**
     * 换输出面 —— 冻结与解冻都走它。
     *
     * ⚠️ **换面本身会触发一次渲染**（静止屏也一样）⇒ 换完必须请求同步帧，否则首帧不是 IDR。
     *
     * @param surface 新输出面；传 null 表示解绑（冻结）
     */
    fun setSurface(surface: Surface?) {
        val vd = virtualDisplay ?: return
        vd.surface = surface
    }

    /**
     * 改屏尺寸或密度。
     *
     * @param width 宽（像素）
     * @param height 高（像素）
     * @param dpi 密度
     * @return 成功，或可恢复级失败
     */
    fun resize(width: Int, height: Int, dpi: Int): Outcome<Unit> {
        val vd = virtualDisplay ?: return Outcome.Ok(Unit)
        return try {
            vd.resize(width, height, dpi)
            Outcome.Ok(Unit)
        } catch (e: Exception) {
            Outcome.Err(Fail(FailLevel.RECOVERABLE, FailCode.SCREEN_CREATE, "改屏尺寸失败", e))
        }
    }

    /**
     * 拆屏。
     *
     * ⚠️ **必须在编码器 `release()` 之前调用**（硬顺序）：反过来的话，编码器的输入面会在屏仍在
     * 渲染时失效。幂等 —— 无屏时是 no-op。
     */
    fun release() {
        val vd = virtualDisplay ?: return
        virtualDisplay = null
        requested = null
        try {
            vd.release()
            ln.i("屏已释放")
        } catch (e: Exception) {
            // 释放失败没有补救手段；系统会在 owner 进程死亡时回收（已实测）。
            ln.w("释放屏时抛异常：${e.message}")
        }
    }

    /**
     * 当前建屏参数。
     *
     * @return 参数快照；无屏时为 null
     */
    fun currentRequest(): Request? = requested

    /**
     * 一次建屏用的参数 —— 用来判断重连时参数有没有变（变了就得重建）。
     *
     * @property name 屏名
     * @property width 宽（像素）
     * @property height 高（像素）
     * @property dpi 密度
     * @property showSystemDecorations 是否显示系统装饰
     * @property alwaysUnlocked 是否保持解锁态
     */
    class Request(
        val name: String,
        val width: Int,
        val height: Int,
        val dpi: Int,
        val showSystemDecorations: Boolean,
        val alwaysUnlocked: Boolean,
    )
}
