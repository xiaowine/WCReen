package com.xiaowine.wcreen.agent.wrappers

import android.app.Application
import android.app.Instrumentation
import android.content.AttributionSource
import android.content.Context
import android.content.pm.ApplicationInfo
import android.os.Process
import com.xiaowine.wcreen.agent.runtime.Fail
import com.xiaowine.wcreen.agent.runtime.FailCode
import com.xiaowine.wcreen.agent.runtime.FailLevel
import com.xiaowine.wcreen.agent.runtime.Ln
import com.xiaowine.wcreen.agent.runtime.Outcome
import java.lang.reflect.Constructor
import java.lang.reflect.Field
import java.lang.reflect.Method

/**
 * 隐藏 API 域：`android.app.ActivityThread` 与它的几个非 public 成员。
 *
 * 为什么躲不掉：`app_process` 里没有 Application，而 `Context.getSystemService()` 拿到的
 * 系统 Context 绑的是真系统身份；**取系统 Context 的唯一路径**就是造一个 `ActivityThread`，
 * 而它的构造器与字段是 private / package-private ⇒ 只能反射。
 * （`ActivityThread` 这个**类**本身也不在 SDK 的 `android.jar` 里 ⇒ 连类型都引用不了。）
 *
 * 三条硬要求（见 `docs/spec/agent/display.md` §2.2，任一条不满足都表现为"莫名其妙的失败"）：
 *  1. **主线程必须先有 Looper 再构造 `ActivityThread`** —— 它构造里会 `new Handler`；
 *  2. 必须填 `mConfigurationController`（SDK ≥ 31），否则 `getSystemContext()` 取 Resources 时 NPE；
 *  3. 包名必须伪装成 `com.android.shell`（与进程真实 uid 2000 一致）。
 *
 * ⚠️ **不要给 `AppBindData.appInfo.targetSdkVersion` 赋值** —— 保持 0，而隐藏 API 限制只对
 * `targetSdk >= 28` 生效，所以整个过程天然不受黑名单阻拦（这也是不需要 `AndroidHiddenApiBypass` 的原因）。
 *
 * 按反射纪律：成员名只在本文件出现，启动时解析一次并缓存，对外只暴露 `Context`。
 */
internal object SystemContext {

    /** `ActivityThread` 的类名。 */
    private const val ACTIVITY_THREAD = "android.app.ActivityThread"

    /** `AppBindData` 的类名。 */
    private const val APP_BIND_DATA = "android.app.ActivityThread\$AppBindData"

    /** `ConfigurationController` 的类名。 */
    private const val CONFIGURATION_CONTROLLER = "android.app.ConfigurationController"

    /** 伪装的包名 —— 与进程真实 uid（2000）一致。 */
    const val PACKAGE_NAME: String = "com.android.shell"

    /** 解析出来的成员集合。 */
    private class Members(
        val activityThreadClass: Class<*>,
        val ctor: Constructor<*>,
        val sCurrentActivityThread: Field,
        val mSystemThread: Field,
        val mConfigurationController: Field,
        val configurationControllerCtor: Constructor<*>,
        val mBoundApplication: Field,
        val appBindDataCtor: Constructor<*>,
        val appBindDataAppInfo: Field,
        val getSystemContext: Method,
        val mInitialApplication: Field?,
    )

    /** 解析结果；null 表示尚未解析或解析失败。 */
    private var members: Members? = null

    /**
     * 解析全部成员。
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

        val atClass = need(ACTIVITY_THREAD) { Class.forName(ACTIVITY_THREAD) }
        val ctor = need("$ACTIVITY_THREAD()") {
            requireNotNull(atClass).getDeclaredConstructor().also { it.isAccessible = true }
        }
        val sCurrent = need("$ACTIVITY_THREAD#sCurrentActivityThread") {
            requireNotNull(atClass).getDeclaredField("sCurrentActivityThread").also { it.isAccessible = true }
        }
        val mSystemThread = need("$ACTIVITY_THREAD#mSystemThread") {
            requireNotNull(atClass).getDeclaredField("mSystemThread").also { it.isAccessible = true }
        }
        val mConfigController = need("$ACTIVITY_THREAD#mConfigurationController") {
            requireNotNull(atClass).getDeclaredField("mConfigurationController").also { it.isAccessible = true }
        }
        val ccClass = need(CONFIGURATION_CONTROLLER) { Class.forName(CONFIGURATION_CONTROLLER) }
        val ccCtor = need("$CONFIGURATION_CONTROLLER(ActivityThread)") {
            val clazz = requireNotNull(ccClass)
            // 优先取参数是 ActivityThread 的那个；取不到就退回唯一构造器。
            (clazz.declaredConstructors.firstOrNull { it.parameterTypes.size == 1 } ?: clazz.declaredConstructors.first())
                .also { it.isAccessible = true }
        }
        val mBoundApplication = need("$ACTIVITY_THREAD#mBoundApplication") {
            requireNotNull(atClass).getDeclaredField("mBoundApplication").also { it.isAccessible = true }
        }
        val abdClass = need(APP_BIND_DATA) { Class.forName(APP_BIND_DATA) }
        val abdCtor = need("$APP_BIND_DATA()") {
            requireNotNull(abdClass).getDeclaredConstructor().also { it.isAccessible = true }
        }
        val abdAppInfo = need("$APP_BIND_DATA#appInfo") {
            requireNotNull(abdClass).getDeclaredField("appInfo").also { it.isAccessible = true }
        }
        val getSystemContext = need("$ACTIVITY_THREAD#getSystemContext()") {
            requireNotNull(atClass).getDeclaredMethod("getSystemContext").also { it.isAccessible = true }
        }

        if (missing.isNotEmpty()) {
            return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.WRAPPER_MISSING,
                    "系统 Context 引导缺少 ${missing.size} 个隐藏成员：${missing.joinToString(", ")}",
                ),
            )
        }

        // mInitialApplication 是 best-effort：它只在 getSystemContext() **之后**才需要，
        // 且拿不到也不影响取系统 Context ⇒ 解析失败不算启动失败，只记一条日志。
        val mInitialApplication = try {
            atClass!!.getDeclaredField("mInitialApplication").also { it.isAccessible = true }
        } catch (e: Exception) {
            null
        }

        members = Members(
            activityThreadClass = atClass!!,
            ctor = ctor!!,
            sCurrentActivityThread = sCurrent!!,
            mSystemThread = mSystemThread!!,
            mConfigurationController = mConfigController!!,
            configurationControllerCtor = ccCtor!!,
            mBoundApplication = mBoundApplication!!,
            appBindDataCtor = abdCtor!!,
            appBindDataAppInfo = abdAppInfo!!,
            getSystemContext = getSystemContext!!,
            mInitialApplication = mInitialApplication,
        )
        return Outcome.Ok(Unit)
    }

    /**
     * 造出可用的系统 Context。
     *
     * **必须在任何 `DisplayManager` / `PackageManager` 调用之前执行**，且调用方线程必须已经
     * `Looper.prepare()` 过（见 `display/Bootstrap.kt`）。
     *
     * @param ln 日志出口
     * @return 系统 Context，或**启动级**失败
     */
    fun install(ln: Ln): Outcome<Context> {
        val m = members ?: return Outcome.Err(
            Fail(FailLevel.FATAL, FailCode.WRAPPER_MISSING, "系统 Context 引导尚未解析"),
        )

        val thread = try {
            m.ctor.newInstance()
        } catch (e: Exception) {
            return Outcome.Err(Fail(FailLevel.FATAL, FailCode.BOOTSTRAP, "构造 ActivityThread 失败", e))
        }

        return try {
            m.sCurrentActivityThread.set(null, thread)
            m.mSystemThread.setBoolean(thread, true)

            // SDK ≥ 31 必须填：getSystemContext() 内部会经 currentActivityThread().getConfiguration()，
            // 该路径要求 mConfigurationController 非 null。
            val cc = m.configurationControllerCtor.newInstance(thread)
            m.mConfigurationController.set(thread, cc)

            // 包名伪装成 com.android.shell（与 uid 2000 一致），否则 system_server 侧权限判定失败。
            // 注意：**不要**碰 targetSdkVersion，保持 0 才能绕开隐藏 API 黑名单。
            val bindData = m.appBindDataCtor.newInstance()
            m.appBindDataAppInfo.set(bindData, ApplicationInfo().also { it.packageName = PACKAGE_NAME })
            m.mBoundApplication.set(thread, bindData)

            val context = m.getSystemContext.invoke(thread) as? Context
                ?: return Outcome.Err(
                    Fail(FailLevel.FATAL, FailCode.BOOTSTRAP, "ActivityThread.getSystemContext() 返回 null"),
                )

            // mInitialApplication 是 best-effort：拿不到也不影响取系统 Context。
            val initial = m.mInitialApplication
            if (initial != null) {
                try {
                    initial.set(thread, Instrumentation.newApplication(Application::class.java, context))
                } catch (e: Exception) {
                    ln.w("填 mInitialApplication 失败（不影响系统 Context）：${e.message}")
                }
            }

            Outcome.Ok(context)
        } catch (e: Exception) {
            Outcome.Err(Fail(FailLevel.FATAL, FailCode.BOOTSTRAP, "系统 Context 引导失败", e))
        }
    }

    /**
     * 把系统 Context 的 **attribution** 改成指定包名（本项目 = `com.android.shell`）。
     *
     * # 为什么必须
     *
     * 虚拟设备（`android.companion.virtual`）的**服务端**按 attribution 的包名判定"这台设备属于谁"，
     * 并据此查 CDM 关联 / 校验权限；而真系统 Context 的 attribution 包名是 `android`，
     * 与本进程真实身份（uid 2000 = `com.android.shell`）对不上 ⇒ 建虚拟设备会被拒。
     *
     * # 为什么 `FakeContext` 改不到它
     *
     * `FakeContext` 覆写的是 `getPackageName()` / `getOpPackageName()`；而 attribution 是
     * **`ContextImpl` 的字段**，由 `getAttributionSource()` 返回，包一层 `ContextWrapper`
     * 改不了它（包装只是把调用委托给 `mBase`）。
     *
     * @param context 真系统 Context（**不是**包装后的那个）
     * @param packageName 目标包名
     * @return 成功，或**启动级**失败（字段不存在 / 回读不符）
     */
    fun patchAttribution(context: Context, packageName: String): Outcome<Unit> {
        val desired = AttributionSource.Builder(Process.myUid())
            .setPackageName(packageName)
            .build()
        try {
            val field = context.javaClass.getDeclaredField("mAttributionSource")
            field.isAccessible = true
            field.set(context, desired)
        } catch (e: Exception) {
            return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.WRAPPER_MISSING,
                    "缺少隐藏字段 ${context.javaClass.name}.mAttributionSource",
                    e,
                ),
            )
        }

        // 「设置参数必须回读确认」：改没改上，只有回读才知道。
        val actual = context.attributionSource
        if (actual.packageName != packageName || actual.uid != Process.myUid()) {
            return Outcome.Err(
                Fail(
                    FailLevel.FATAL,
                    FailCode.BOOTSTRAP,
                    "attribution 回读不符：期望 $packageName/${Process.myUid()}，实际 ${actual.packageName}/${actual.uid}",
                ),
            )
        }
        return Outcome.Ok(Unit)
    }
}
