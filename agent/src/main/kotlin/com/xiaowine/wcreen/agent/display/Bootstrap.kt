package com.xiaowine.wcreen.agent.display

import android.content.Context
import android.os.Looper
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Ln
import com.xiaowine.wcreen.agent.runtime.Outcome
import com.xiaowine.wcreen.agent.wrappers.MainLooper
import com.xiaowine.wcreen.agent.wrappers.SystemContext

/**
 * 进程引导链 —— `main` 编排的第 ② 步，**必须早于任何线程**。
 *
 * 顺序是硬的（`docs/spec/agent/display.md` §2.2）：
 *
 * ```
 * Looper.prepare() → 反射把 Looper.sMainLooper 设为它
 *   → 反射构造 ActivityThread，写入 sCurrentActivityThread、mSystemThread = true
 *   → 填 mConfigurationController（SDK ≥ 31）
 *   → 填 mBoundApplication（appInfo.packageName = com.android.shell）
 *   → 反射调 getSystemContext() 拿到系统 Context
 *   → 包一层伪装包名的 Context 包装
 * ```
 *
 * 用 `Looper.prepare()` 而不是 `prepareMainLooper()`：后者走 `prepare(false)`，
 * 得到的 Looper 不允许退出 ⇒ `quitSafely()` 抛异常、`Looper.loop()` 永不返回，
 * 而收尾序列正是在 `Looper.loop()` 返回之后的 `finally` 里跑的。
 */
internal object Bootstrap {

    /**
     * 跑引导链。
     *
     * @param ln 日志出口（此时等级还是缺省的 VERBOSE —— CLI 在第 ③ 步才解析）
     * @return 伪装包名的系统 Context，或**启动级**失败
     */
    fun run(ln: Ln): Outcome<Context> {
        Looper.prepare()
        val mine = Looper.myLooper()
            ?: return Outcome.Err(
                Fail(FailLevel.FATAL, FailCode.BOOTSTRAP, "Looper.prepare() 之后 myLooper() 仍为 null"),
            )

        when (val r = MainLooper.install(mine)) {
            is Outcome.Ok -> Unit
            is Outcome.Err -> return r
        }

        // 补上 sMainLooper 的唯一目的就是让 getMainLooper() 指向它；顺手回读确认
        // ——「设置参数必须回读确认」在本项目是全局规则。
        if (Looper.getMainLooper() !== mine) {
            return Outcome.Err(
                Fail(FailLevel.FATAL, FailCode.BOOTSTRAP, "登记主 Looper 后 getMainLooper() 仍不是它"),
            )
        }

        when (val r = SystemContext.resolve()) {
            is Outcome.Ok -> Unit
            is Outcome.Err -> return r
        }

        val system = when (val r = SystemContext.install(ln)) {
            is Outcome.Ok -> r.value
            is Outcome.Err -> return r
        }

        // attribution 也要伪装：虚拟设备的**服务端**按 attribution 的包名查 CDM 关联 /
        // 判权限，而真系统 Context 报的是 `android`（见 wrappers/VirtualDevice.kt）。
        when (val r = SystemContext.patchAttribution(system, FakeContext.PACKAGE_NAME)) {
            is Outcome.Ok -> Unit
            is Outcome.Err -> return r
        }

        // 包一层伪装包名：system_server 侧的权限 / 归属判定看的是 Context 包名，
        // 真系统 Context 报 `android`，与 uid 2000 对不上。
        val fake = FakeContext(system)
        if (fake.packageName != FakeContext.PACKAGE_NAME) {
            return Outcome.Err(
                Fail(FailLevel.FATAL, FailCode.BOOTSTRAP, "伪装 Context 的包名仍是 ${fake.packageName}"),
            )
        }

        ln.d("进程引导完成：myLooper == getMainLooper，包名伪装为 ${fake.packageName}")
        return Outcome.Ok(fake)
    }
}
