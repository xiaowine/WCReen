package com.xiaowine.wcreen.agent.display

import android.content.Context
import android.content.ContextWrapper

/**
 * 伪装包名的 Context 包装。
 *
 * 为什么必须伪装：我们以 uid 2000 运行，而 system_server 侧的权限 / 归属判定看的是
 * Context 的包名 —— 真系统 Context 报 `android`，与本进程身份对不上，建屏会失败。
 * 覆写成 `com.android.shell`（本进程真实身份）才对得上。
 *
 * ⚠️ **不能用 `ContextWrapper` 去改 `getSystemService()` 的返回值** —— 它把调用委托给
 * `mBase`，包一层改不了 `DisplayManager` 内部持有的 Context。所以 `DisplayManager`
 * 必须走隐藏构造器显式塞进这个包装（见 `wrappers/DisplayManagerFactory.kt`）。
 *
 * @param base 真正的系统 Context
 */
internal class FakeContext(base: Context) : ContextWrapper(base) {

    /** 进程身份。 */
    override fun getPackageName(): String = PACKAGE_NAME

    /** `getOpPackageName()` 与包名走同一条判定路径，必须一起改。 */
    override fun getOpPackageName(): String = PACKAGE_NAME

    /**
     * 返回自身 —— **虚拟设备的包装层必须能拿到它**。
     *
     * `VirtualDeviceInternal` 构造时会取 attribution，取法经过 `getApplicationContext()`；
     * 而 `app_process` 里没有 Application，真系统 Context 的这个方法返回 **null** ⇒ 建虚拟设备时
     * NPE（`Attempt to invoke virtual method Context.getAttributionSource() on a null object reference`）。
     * 已实测踩过（2026-10-07）。
     */
    override fun getApplicationContext(): Context = this

    /** 与进程真实 uid（2000）一致的包名。 */
    companion object {

        /** 伪装的目标包名。 */
        const val PACKAGE_NAME: String = "com.android.shell"
    }
}
