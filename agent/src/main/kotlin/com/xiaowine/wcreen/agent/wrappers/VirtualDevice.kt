package com.xiaowine.wcreen.agent.wrappers

import android.content.Context
import android.hardware.display.VirtualDisplay
import android.hardware.display.VirtualDisplayConfig
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Outcome
import java.lang.reflect.Constructor
import java.lang.reflect.Method

/**
 * 隐藏 API 域：`android.companion.virtual`（**虚拟设备**）。
 *
 * # 为什么建屏要走虚拟设备
 *
 * 只为一条性质：**不随物理屏锁屏 / 息屏** —— 请求屏位 `ALWAYS_UNLOCKED` 并让设备处于
 * `LOCK_STATE_ALWAYS_UNLOCKED`（`docs/plans/2026-10-07-vdm-mechanism-corrected.md`）。
 *
 * 在**默认设备**上建屏拿不到它：`ALWAYS_UNLOCKED` 只对**非默认显示组**的屏有效
 * （默认组里的屏会被平台**直接丢弃**该位，AOSP `DisplayManagerService.createVirtualDisplayInternal`）。
 * 虚拟设备的屏**天然在设备自己的显示组**里（平台自动加 `DEVICE_DISPLAY_GROUP`）⇒ 解锁位被接受。
 *
 * ⚠️ **"铺满"不是本模块的理由**（2026-10-08 产品决定放弃该目标）：不可缩放 + 固定方向的 App 在
 * 14/15 上可能被裁（fixed-orientation letterbox / size-compat），按平台行为接受；16+ 由
 * [ActivitySizeRestrictions] 取消活动级 letterbox。机制与留档见
 * `docs/spec/agent/display-letterbox.md`。
 *
 * # 客户端层签名（14 / 15 / 16 一致，实测）
 *
 * ```
 * VirtualDeviceManager(IVirtualDeviceManager$Stub.asInterface(getService("virtualdevice")), ctx)
 *   .createVirtualDevice(associationId, VirtualDeviceParams)          → VirtualDevice
 *   .createVirtualDisplay(VirtualDisplayConfig, Executor, Callback)   → VirtualDisplay
 * ```
 *
 * # 两个前置条件（都实测踩过）
 *
 * 1. **CDM 关联**：服务端要求"设备必须有对应关联"（见 `display/CompanionAssociation.kt`）；
 * 2. **attribution 包名 = `com.android.shell`**：真系统 Context 报 `android`，与本进程身份（uid 2000）
 *    对不上 ⇒ 由 [SystemContext.patchAttribution] 补上。
 *
 * # 反射纪律
 *
 * 成员名只在本文件出现；启动时解析一次并缓存（[resolve]），对外只暴露普通 Kotlin 类型
 * （[Handle] / `VirtualDisplay`）⇒ 调用方（`display/Screen.kt`）拿不到任何 `Class`。
 */
internal object VirtualDevice {

    /** 管理器的类名。 */
    private const val MANAGER = "android.companion.virtual.VirtualDeviceManager"

    /** 管理器的 Binder 接口类型名（隐藏构造器的第一个参数）。 */
    private const val MANAGER_IFACE = "android.companion.virtual.IVirtualDeviceManager"

    /** Binder 接口的 `Stub`（提供 `asInterface`）。 */
    private const val MANAGER_STUB = "android.companion.virtual.IVirtualDeviceManager\$Stub"

    /** 设备客户端（`createVirtualDisplay` 在它上面）。 */
    private const val DEVICE = "android.companion.virtual.VirtualDeviceManager\$VirtualDevice"

    /** 设备参数与其 Builder。 */
    private const val PARAMS = "android.companion.virtual.VirtualDeviceParams"
    private const val PARAMS_BUILDER = "android.companion.virtual.VirtualDeviceParams\$Builder"

    /** `ServiceManager` 与要取的服务名。 */
    private const val SERVICE_MANAGER = "android.os.ServiceManager"
    private const val SERVICE_NAME = "virtualdevice"

    /**
     * 解析出来的成员集合。
     *
     * @property managerCtor `VirtualDeviceManager(IVirtualDeviceManager, Context)`
     * @property asInterface `IVirtualDeviceManager$Stub.asInterface(IBinder)`
     * @property getService `ServiceManager.getService(String)`
     * @property builderCtor `VirtualDeviceParams$Builder()`
     * @property setName `Builder.setName(String)`
     * @property setLockState `Builder.setLockState(int)`
     * @property build `Builder.build()`
     * @property lockStateAlwaysUnlocked `LOCK_STATE_ALWAYS_UNLOCKED` 常量值
     * @property createVirtualDevice `VirtualDeviceManager.createVirtualDevice(int, VirtualDeviceParams)`
     * @property createVirtualDisplay `VirtualDevice.createVirtualDisplay(VirtualDisplayConfig, Executor, Callback)`
     */
    private class Members(
        val managerCtor: Constructor<*>,
        val asInterface: Method,
        val getService: Method,
        val builderCtor: Constructor<*>,
        val setName: Method,
        val setLockState: Method,
        val build: Method,
        val lockStateAlwaysUnlocked: Int,
        val createVirtualDevice: Method,
        val createVirtualDisplay: Method,
    )

    /** 解析结果；null 表示尚未解析或解析失败。 */
    private var members: Members? = null

    /**
     * 一台已创建的虚拟设备。
     *
     * 只做**类型封装**：对外不暴露 `Class`，内部持有反射得到的实例。
     *
     * @property raw 反射拿到的设备实例
     */
    internal class Handle internal constructor(val raw: Any)

    /**
     * 解析全部成员（启动期调用一次）。
     *
     * @return 成功，或**启动级**失败并**列出缺哪几条**
     */
    fun resolve(): Outcome<Unit> {
        val missing = ArrayList<String>()

        /** 逐个解析并记录缺失的名字，最后一次性报出来。 */
        fun <T> need(name: String, block: () -> T): T? = try {
            block()
        } catch (e: Exception) {
            missing += name
            null
        }

        val managerClass = need(MANAGER) { Class.forName(MANAGER) }
        val ifaceClass = need(MANAGER_IFACE) { Class.forName(MANAGER_IFACE) }
        val stubClass = need(MANAGER_STUB) { Class.forName(MANAGER_STUB) }
        val deviceClass = need(DEVICE) { Class.forName(DEVICE) }
        val paramsClass = need(PARAMS) { Class.forName(PARAMS) }
        val builderClass = need(PARAMS_BUILDER) { Class.forName(PARAMS_BUILDER) }
        val serviceManagerClass = need(SERVICE_MANAGER) { Class.forName(SERVICE_MANAGER) }

        val managerCtor = need("$MANAGER(IVirtualDeviceManager, Context)") {
            managerClass!!.getDeclaredConstructor(ifaceClass, Context::class.java)
                .also { it.isAccessible = true }
        }
        val asInterface = need("$MANAGER_STUB.asInterface(IBinder)") {
            stubClass!!.getMethod("asInterface", android.os.IBinder::class.java)
        }
        val getService = need("$SERVICE_MANAGER.getService(String)") {
            serviceManagerClass!!.getMethod("getService", String::class.java)
        }
        val builderCtor = need("$PARAMS_BUILDER()") {
            builderClass!!.getDeclaredConstructor().also { it.isAccessible = true }
        }
        val setName = need("$PARAMS_BUILDER.setName(String)") {
            builderClass!!.getMethod("setName", String::class.java)
        }
        val setLockState = need("$PARAMS_BUILDER.setLockState(int)") {
            builderClass!!.getMethod("setLockState", Int::class.javaPrimitiveType)
        }
        val build = need("$PARAMS_BUILDER.build()") { builderClass!!.getMethod("build") }
        val lockStateAlwaysUnlocked = need("$PARAMS.LOCK_STATE_ALWAYS_UNLOCKED") {
            paramsClass!!.getDeclaredField("LOCK_STATE_ALWAYS_UNLOCKED")
                .also { it.isAccessible = true }.getInt(null)
        }
        val createVirtualDevice = need("$MANAGER.createVirtualDevice(int, VirtualDeviceParams)") {
            managerClass!!.getMethod(
                "createVirtualDevice", Int::class.javaPrimitiveType, paramsClass,
            )
        }
        val createVirtualDisplay = need(
            "$DEVICE.createVirtualDisplay(VirtualDisplayConfig, Executor, Callback)",
        ) {
            deviceClass!!.getMethod(
                "createVirtualDisplay",
                VirtualDisplayConfig::class.java,
                java.util.concurrent.Executor::class.java,
                VirtualDisplay.Callback::class.java,
            )
        }

        if (missing.isNotEmpty()) {
            return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.WRAPPER_MISSING,
                    "缺少 ${missing.size} 个虚拟设备成员：${missing.joinToString(", ")}",
                ),
            )
        }

        members = Members(
            managerCtor = managerCtor!!,
            asInterface = asInterface!!,
            getService = getService!!,
            builderCtor = builderCtor!!,
            setName = setName!!,
            setLockState = setLockState!!,
            build = build!!,
            lockStateAlwaysUnlocked = lockStateAlwaysUnlocked!!,
            createVirtualDevice = createVirtualDevice!!,
            createVirtualDisplay = createVirtualDisplay!!,
        )
        return Outcome.Ok(Unit)
    }

    /**
     * 创建一台虚拟设备。
     *
     * @param context **伪装过包名与 attribution** 的系统 Context（见 `display/Bootstrap.kt`）
     * @param associationId CDM 关联号（见 `display/CompanionAssociation.kt`）
     * @param name 设备名（诊断用，会出现在 `dumpsys` 里）
     * @param alwaysUnlocked 是否要求"恒不锁"（`LOCK_STATE_ALWAYS_UNLOCKED`）
     * @return 设备句柄，或**启动级**失败
     */
    fun create(
        context: Context,
        associationId: Int,
        name: String,
        alwaysUnlocked: Boolean,
    ): Outcome<Handle> {
        val m = members ?: return Outcome.Err(
            Fail(FailLevel.FATAL, FailCode.WRAPPER_MISSING, "虚拟设备成员尚未解析"),
        )
        return try {
            val binder = m.getService.invoke(null, SERVICE_NAME)
            val iface = m.asInterface.invoke(null, binder)
            val manager = m.managerCtor.newInstance(iface, context)
            val builder = m.builderCtor.newInstance()
            m.setName.invoke(builder, name)
            if (alwaysUnlocked) {
                m.setLockState.invoke(builder, m.lockStateAlwaysUnlocked)
            }
            val params = m.build.invoke(builder)
            val raw = m.createVirtualDevice.invoke(manager, associationId, params)
                ?: return Outcome.Err(
                    Fail(FailLevel.FATAL, FailCode.ASSOCIATION, "createVirtualDevice 返回 null"),
                )
            Outcome.Ok(Handle(raw))
        } catch (e: Exception) {
            Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.ASSOCIATION,
                    "创建虚拟设备失败（关联号 $associationId）：${e.message}",
                    e,
                ),
            )
        }
    }

    /**
     * 在设备上挂一块屏。
     *
     * @param handle 设备句柄
     * @param config 建屏配置（公开 `VirtualDisplayConfig.Builder` 装配）
     * @return 屏，或**可恢复级**失败（资源类，PC 可稍后重试）
     */
    fun createDisplay(handle: Handle, config: VirtualDisplayConfig): Outcome<VirtualDisplay> {
        val m = members ?: return Outcome.Err(
            Fail(FailLevel.FATAL, FailCode.WRAPPER_MISSING, "虚拟设备成员尚未解析"),
        )
        return try {
            val vd = m.createVirtualDisplay.invoke(handle.raw, config, null, null) as VirtualDisplay?
            if (vd == null) {
                Outcome.Err(
                    Fail(
                        FailLevel.RECOVERABLE,
                        FailCode.SCREEN_CREATE,
                        "建屏返回 null（${config.name} ${config.width}x${config.height}）—— 平台拒绝或资源不足",
                    ),
                )
            } else {
                Outcome.Ok(vd)
            }
        } catch (e: Exception) {
            Outcome.Err(
                Fail(
                    FailLevel.RECOVERABLE,
                    FailCode.SCREEN_CREATE,
                    "建屏失败（${config.name} ${config.width}x${config.height}）：${e.message}",
                    e,
                ),
            )
        }
    }
}
