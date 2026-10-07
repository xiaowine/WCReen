package com.xiaowine.wcreen.agent.display

import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Ln
import com.xiaowine.wcreen.agent.runtime.Outcome

/**
 * CDM（Companion Device Manager）**关联** —— 虚拟设备的硬前置。
 *
 * # 为什么需要
 *
 * `VirtualDeviceManager.createVirtualDevice(associationId, …)` 要求"这台设备必须有一条对应的
 * CDM 关联"；关联缺失时创建会被服务端拒绝。关联是**系统级持久状态**（用户可在系统设置里撤销），
 * 所以每次启动都要**自检**，缺了就重建（幂等）。
 *
 * # 取法：走 `cmd`（shell 命令），不走反射
 *
 * 本项目以 uid 2000（shell）运行 ⇒ 可以直接执行系统自带的 `cmd companiondevice`：
 *
 * ```
 * cmd companiondevice list 0                                  # 读现有
 * cmd companiondevice associate 0 com.android.shell <mac> \
 *     android.app.role.COMPANION_DEVICE_APP_STREAMING SELF_MANAGED   # 缺则建
 * ```
 *
 * 不反射 `CompanionDeviceManager` 的隐藏 API：那些接口面向"应用内发起配对"，需要 Activity /
 * 回调 / 用户确认，对本用途（自管理关联）既重又脆。
 *
 * # 关联形态（本机实测）
 *
 * `Association{mId=1, mPackageName='com.android.shell', mDeviceProfile='…APP_STREAMING',
 * mSelfManaged=true, mRevoked=false}`
 */
internal object CompanionAssociation {

    /** `cmd` 的位置。 */
    private const val CMD = "/system/bin/cmd"

    /** 关联归属的包名 —— 与进程真实身份（uid 2000）一致。 */
    private const val PACKAGE_NAME = FakeContext.PACKAGE_NAME

    /**
     * 关联用的 MAC —— **本用途不需要真实设备**，只要一条稳定的自管理关联即可
     * （本机实测用的就是这一条；换设备后按同一条重建）。
     */
    private const val MAC = "fa:ce:fe:ed:f0:18"

    /** 设备 profile：应用投屏。 */
    private const val PROFILE = "android.app.role.COMPANION_DEVICE_APP_STREAMING"

    /** 自管理关联（无需用户配对流程）。 */
    private const val SELF_MANAGED = "SELF_MANAGED"

    /** 用户号：本项目的屏建在系统用户下。 */
    private const val USER_ID = "0"

    /**
     * 确保关联存在，并返回它的 id。
     *
     * @param ln 日志出口
     * @return 关联号，或**启动级**失败（拿不到关联 ⇒ 建不了虚拟设备）
     */
    fun ensure(ln: Ln): Outcome<Int> {
        val existing = run("companiondevice", "list", USER_ID)
            ?: return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.ASSOCIATION,
                    "执行 `cmd companiondevice list` 失败（拿不到 CDM 关联）",
                ),
            )

        parse(existing)?.let {
            ln.d("CDM 关联已存在：id=$it（$PACKAGE_NAME）")
            return Outcome.Ok(it)
        }

        ln.i("CDM 关联缺失 ⇒ 新建（$PACKAGE_NAME / $PROFILE / $SELF_MANAGED）")
        run(
            "companiondevice", "associate", USER_ID, PACKAGE_NAME, MAC, PROFILE, SELF_MANAGED,
        ) ?: return Outcome.Err(
            Fail(FailLevel.FATAL, FailCode.ASSOCIATION, "执行 `cmd companiondevice associate` 失败"),
        )

        // 「设置参数必须回读确认」：新建之后必须**重新读一遍**并拿到 id。
        val after = run("companiondevice", "list", USER_ID)
            ?: return Outcome.Err(
                Fail(FailLevel.FATAL, FailCode.ASSOCIATION, "新建关联后无法回读"),
            )
        return parse(after)?.let { Outcome.Ok(it) } ?: Outcome.Err(
            Fail(
                FailLevel.FATAL,
                FailCode.ASSOCIATION,
                "新建关联后仍读不到 $PACKAGE_NAME 的关联（关联可能被系统拒绝）",
            ),
        )
    }

    /**
     * 从 `list` 的输出里取出本包的关联号。
     *
     * 输出形如（表头两行 + 每条一行）：
     * ```
     * Max ID: 1
     * Association ID | Package Name | Mac Address
     * 1 | com.android.shell | fa:ce:fe:ed:f0:18
     * ```
     *
     * @param output 命令输出
     * @return 关联号；没有本包的关联时 null
     */
    private fun parse(output: String): Int? = output.lineSequence()
        .map { it.split('|').map(String::trim) }
        .firstOrNull { it.size >= 2 && it[1] == PACKAGE_NAME }
        ?.get(0)
        ?.toIntOrNull()

    /**
     * 跑一条 `cmd` 子命令。
     *
     * @param args `companiondevice` 之后的参数
     * @return 标准输出（含 stderr）；执行失败时 null
     */
    private fun run(vararg args: String): String? = try {
        val process = ProcessBuilder(CMD, *args)
            .redirectErrorStream(true)
            .start()
        val output = process.inputStream.bufferedReader().use { it.readText() }
        if (process.waitFor() == 0) output else null
    } catch (e: Exception) {
        null
    }
}
