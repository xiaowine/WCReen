# `:agent` · 隐藏 API 的访问形态

> 属 [`agent/code.md`](code.md) §10 的展开；**写 `wrappers/` 之前必读**。
> 相关：[`agent/module.md`](module.md) §4（依赖政策）/ §8（禁忌）、[`agent/display.md`](display.md) §2.2（引导链）。
> **变更记录记在本文件 §6。**

**状态**：**已定**（2026-10-07，**用户已确认**）。

---

## 1. 原则

**尽可能不用反射。** 顺序固定：

1. 能用**公开 API** ⇒ 用公开 API（§2）；
2. 非隐藏 API 不可 ⇒ **反射**，且**只允许在 `wrappers/` 出现**（[`agent/code.md`](code.md) §2.1）。

**不引入「补全 hidden 的 android.jar」** —— 已实测否决，理由与证据见 §5。

---

## 2. 走公开 API 的（**不得反射**）

| 域 | 公开 API |
|---|---|
| 建屏 / 换面 / 冻结 | **公开**的 `VirtualDisplayConfig.Builder(name, w, h, dpi).setFlags(...).setSurface(...)` + `VirtualDisplay.setSurface()` / `resize()` / `release()`；**建屏这个动作本身走虚拟设备**（隐藏，见 §3 第 7 处）—— ⚠️ 配置上的「忽略活动尺寸限制」也是**隐藏**方法，见 §3 第 6 处 |
| 编码 | `MediaCodec` / `MediaCodecList`（[`agent/encode-probe.md`](encode-probe.md) §11：**不需要隐藏 API**） |
| 枚举 | `PackageManager` / `ApplicationPackageManager` |
| 拉起 | `cmd activity --display` 子进程（[`launch.md`](launch.md) §4.2） |
| 传输 | `LocalServerSocket` / `ServerSocket` |
| 伪装 Context | `ContextWrapper` 覆写 `getPackageName()` / `getOpPackageName()` |

---

## 3. 非反射不可的（**运行期 7 处**，全部登记）

| # | 成员 | 为什么躲不掉 | 出处 |
|---|---|---|---|
| 1 | `Looper.sMainLooper`（**字段**） | 公开的 `prepareMainLooper()` 会 `quitAllowed=false` ⇒ 主线程**退不掉**，收尾的 `finally` 永不执行 | [`agent/display.md`](display.md) §2.2 |
| 2 | `ActivityThread` 构造器 + `sCurrentActivityThread` / `mSystemThread` / `mConfigurationController` / `mBoundApplication` / `getSystemContext()` / `mInitialApplication` | `app_process` 取系统 Context 的**唯一路径**；这些字段是 private / package-private | 同上 |
| 3 | `DisplayManager(Context)` **隐藏构造器** | 公开的 `getSystemService()` 返回的实例绑的是**真系统 Context**（包名 `android`）⇒ `SecurityException: packageName must match the calling uid` | 同上 |
| 4 | `ActivityTaskManager.getService()` → `IActivityTaskManager.getAllRootTaskInfosOnDisplay(int)` | 拉起判据；唯一替代是解析 `dumpsys activity activities`，其**段顺序随聚焦屏变化** | [`launch.md`](launch.md) §4.3 |
| 5 | 输入注入（`InputManager.injectInputEvent` + `InputEvent.setDisplayId` + 模式常量） | 公开 API **没有**注入能力，也没有设 displayId 的入口（按键事件尤其） | [`input.md`](input.md) §1 |
| 6 | `VirtualDisplayConfig.Builder#setIgnoreActivitySizeRestrictions(boolean)`（**Android 16 起才有**） | 它是"活动的尺寸限制"（= **活动级 letterbox**）的**唯一解除入口**：屏不会为 App 的固定方向请求旋转（VDM 建的屏实测 `mFixedToUserRotation=true`）⇒ **不可缩放 + 固定方向**的 App 被裁。**14/15 没有它** ⇒ 这类 App 会被裁（产品决定**不为此另做动作**，见 [`display-letterbox.md`](display-letterbox.md) §6） | [`display-letterbox.md`](display-letterbox.md) · [`display-flags.md`](display-flags.md) §4 |
| 7 | **虚拟设备域**：`ServiceManager.getService("virtualdevice")` + `IVirtualDeviceManager$Stub.asInterface` + `VirtualDeviceManager(IVirtualDeviceManager, Context)` 隐藏构造器 + `VirtualDeviceParams$Builder`（`setName` / `setLockState` / `build` / `LOCK_STATE_ALWAYS_UNLOCKED`）+ `createVirtualDevice` + `VirtualDevice.createVirtualDisplay` + **`ContextImpl.mAttributionSource` 字段** | 屏必须挂在**虚拟设备**上，`ALWAYS_UNLOCKED`（"锁屏/息屏不接管"）才生效 —— 该位只对**非默认组**的屏有效，默认组里的屏会被平台**直接丢弃**该位。该域整个是 `@SystemApi`/隐藏类 ⇒ 全反射；`mAttributionSource` 是 `ContextImpl` 的**字段**（服务端按 attribution 包名查 CDM 关联，真系统 Context 报 `android`） | [`display.md`](display.md) §2 · [`plans/2026-10-07-vdm-mechanism-corrected.md`](../../plans/2026-10-07-vdm-mechanism-corrected.md) §3 |

> ⚠️ **不在运行期清单里的**：`DisplayInfo` 的色彩字段（`getColorMode()` / `getSupportedColorModes()`）。
> 它只在**能力探测**时需要，且结论已固化成规则（虚拟屏是 SDR ⇒ `BT709 + FULL + SDR_VIDEO`）
> ⇒ **运行期不读**（[`agent/encode-capability.md`](encode-capability.md) §1.3 / §5）。

**第 5 处的取法（S4 实测定稿，实体机 Android 16 / SDK 36）**：

| 环节 | 手段 |
|---|---|
| 取 `InputManager` | **公开**的 `getSystemService(INPUT_SERVICE)` —— **0 反射**（`Context.INPUT_SERVICE` 本身也是公开常量） |
| 注入 | 反射 `InputManager.injectInputEvent(InputEvent, int)` |
| 定向到虚拟屏 | 反射 `InputEvent.setDisplayId(int)` —— `IInputManager` 的签名里**没有** displayId 形参，定向只能靠事件自身 |
| 模式常量 | 反射 `INJECT_INPUT_EVENT_MODE_ASYNC` —— **绝不硬编码**（实测 `ASYNC=0 / WAIT_FOR_RESULT=1 / WAIT_FOR_FINISH=2`，而 `input.md` §4 原表把 `WAIT_FOR_FINISH` 记成 `1` ✗） |
| 事件本体 | 全走**公开 API**：`MotionEvent.obtain` 的公开重载、`KeyEvent` 公开构造器 |

⇒ 这一处的反射面**恰好 3 个成员**。**没有**退回 `ServiceManager` → `IInputManager`（那条路要 3 个以上成员，
且多一层 AIDL 存根）。交叉验证：scrcpy 的 `wrappers/InputManager.java` 走同一条路（同样反射
`injectInputEvent` 与 `setDisplayId`，并用伪装 Context 取 `InputManager`）。

⚠️ **为什么 `getSystemService` 这一处能用、而第 3 处（`DisplayManager`）不能用**：
那处卡的是**建屏时的归属校验**（拿 Context 的包名与调用 uid 比对，真系统 Context 报 `android`）；
注入这一路看的是**调用进程的 uid**（`INJECT_EVENTS` 权限），与 Context 的包名无关。

**第 6 处的取法（2026-10-07 实测定稿，Android 16 / SDK 36）**：

| 环节 | 手段 |
|---|---|
| 装配配置 | **公开**的 `VirtualDisplayConfig.Builder(name, w, h, dpi).setFlags(...).setSurface(...)`（SDK 37 起该类与 Builder 均为公开 API） |
| 取消尺寸限制 | 反射 **`Builder.setIgnoreActivitySizeRestrictions(boolean)`** —— 唯一入口；**Android 16 起才有**（`api-versions.xml` 无、设备 `framework.jar` 有；同类工具 AndroMeld 正是把它门控在 `SDK_INT >= 36` 且 `catch LinkageError`） |
| 降级 | 拿不到（Android 14/15）或调用失败 ⇒ **只告警一次**，建屏照常，回到平台默认行为（这类 App 可能被裁，见 [`display-letterbox.md`](display-letterbox.md) §6） |

⇒ **它是清单里第一处"可降级"的反射**：§4 纪律 2「失败即 fatal」**不适用**于它 —— 它买到的是
"任意尺寸都不被裁"，丢了只是回到平台默认行为（这类 App 可能被裁），不该因此拒绝启动。

**第 7 处的取法（2026-10-07 实测定稿，真机 Android 16 / SDK 36 + AVD Android 15 / SDK 35 双端）**：

| 环节 | 手段 |
|---|---|
| 取 Binder 接口 | 反射 `ServiceManager.getService("virtualdevice")` + `IVirtualDeviceManager$Stub.asInterface(IBinder)` |
| 造管理器 | 反射隐藏构造器 `VirtualDeviceManager(IVirtualDeviceManager, Context)` —— 公开的 `getSystemService` 拿不到它（`Context.VIRTUAL_DEVICE_SERVICE` 也不在 SDK 里） |
| 建设备 | `VirtualDeviceParams$Builder` 反射构造 + `setName` + `setLockState(LOCK_STATE_ALWAYS_UNLOCKED)` + `build`，再 `createVirtualDevice(int, VirtualDeviceParams)` |
| 挂屏 | 设备实例上反射 `createVirtualDisplay(VirtualDisplayConfig, Executor, Callback)`（**客户端层签名 14/15/16 一致**，差异被包装层吸收） |
| attribution 伪装 | 反射写 `ContextImpl.mAttributionSource` 字段（`AttributionSource.Builder(uid).setPackageName("com.android.shell")`），并**回读** `Context.getAttributionSource()` 确认 |
| CDM 关联 | **不走反射**：以 uid 2000 执行 `cmd companiondevice list/associate`（见 `display/CompanionAssociation.kt`） |
| 失败形态 | 成员缺失 ⇒ **fatal**（列出缺哪条）；设备/屏创建失败 ⇒ 见 `display.md` §3 |

⚠️ 另有一条**非反射但同样硬**的前置：伪装 Context 必须覆写 `getApplicationContext()` 返回自身 ——
`VirtualDeviceInternal` 构造时经它取 attribution，而 `app_process` 里没有 Application，真系统 Context
返回 **null** ⇒ 建虚拟设备时 NPE（实测踩过）。

---

## 4. 反射的四条纪律

1. **只在启动时解析一次**（类 / 构造器 / 方法 / 字段），结果缓存成 `Method` / `Field` / `Constructor`
   ⇒ 运行期不再 lookup（一致 + 快）；
2. **解析失败 = 启动失败**（退出码 `2`），并**列出缺哪一条**
   ⇒ 把"用到才发现"前移成"**启动就发现、一次看全**"（[`agent/runtime-startup.md`](runtime-startup.md) §5）；
3. **成员名与签名只在一处出现** —— 一个隐藏 API 域一个对象；对外只暴露普通 Kotlin 类型，
   不泄露 `Method` / `Object` / `Class`；**版本差异在此收敛**（[`agent/module.md`](module.md) §8）；
4. **能纯算的切出来** —— 如 flags 位运算与组合**不得**依赖 Android 类型 ⇒ 才能进 `agent/src/test/`
   （[`agent/code.md`](code.md) §8）。

---

## 5. 为什么不用「补全 hidden 的 android.jar」（**已实测否决**）

曾评估 [`Reginer/aosp-android-jar`](https://github.com/Reginer/aosp-android-jar)（预编译 jar，MIT）
与 [`anggrayudi/android-hidden-api`](https://github.com/anggrayudi/android-hidden-api)（自建 `hiddenjar` CLI）
两条路，**均否决**。四条实测结论：

| # | 结论 |
|---|---|
| 1 | **直接调用隐藏 API 在本进程不被拦**（`app_process` / uid 2000 实测：直接调 `ServiceManager.getService` 成功）⇒ 运行期**不需要** `AndroidHiddenApiBypass`（记为退路） |
| 2 | **`compileOnly` 被 SDK 的 `android.jar` 遮蔽，且是「类级」遮蔽** —— SDK 里**存在**的类（`Looper` / `DisplayManager` / `InputManager`），其隐藏成员一律不可见 |
| 3 | **配置层无解** —— SDK 的 `android.jar` **不在任何 dependency configuration 里**（AGP 单独注入），且 AGP 9.3.3 没有 `androidApis` 配置 |
| 4 | **private / package-private 成员，语言层就不许访问** ⇒ 即便替换 jar 也**只能反射** ⇒ 补全 jar 换不到那几处 |

⇒ 补全 jar 的**净收益只有 1 处**（第 3 点），代价却是**机器级改动**（替换 SDK 的 `android.jar`，
影响本机所有项目）或额外一层影子 SDK ⇒ **不划算**。

**保留的实测数据**见 [`plans/2026-10-07-agent-hidden-api.md`](../../plans/2026-10-07-agent-hidden-api.md)：
该文件的**方案部分已被本文件取代**，其 §3 的实测结论**继续有效**。

---

## 6. 变更记录

- **2026-10-07 定稿**：隐藏 API 访问形态落盘。**原则 = 尽可能不用反射**；走公开 API 的 6 个域；
  **运行期反射只保留 5 处**并逐条登记「为什么躲不掉」；
  **四条纪律**（启动时一次解析 + 失败即 fatal 并列出缺哪条 + 名字只在一处 + 能纯算的切出来）；
  **否决补全 jar** 并记录四条实测依据；`DisplayInfo` 色彩字段**移出运行期清单**（降为探测期需要）。
- **2026-10-07 确认**：用户确认本形态 ⇒ 状态转「**已定**」。
  唯一保留的实施期待定项：§3 第 5 处（输入注入）的**取法**（公开 `getSystemService(INPUT_SERVICE)`
  + 只反射 `injectInputEvent`，或退回 `ServiceManager` → `IInputManager`）—— **待 S4（F5）实测定稿**。
- **2026-10-07 第 5 处取法定稿（S4）**：§3 第 5 处落定 —— **公开 `getSystemService(INPUT_SERVICE)`
  零反射**取 `InputManager`，反射面**恰好 3 个成员**（`injectInputEvent` 方法、`InputEvent.setDisplayId`
  方法、`INJECT_INPUT_EVENT_MODE_ASYNC` 常量），事件本体全公开 API；
  **未**退回 `ServiceManager` → `IInputManager`。
  顺带说明「为什么第 3 处不行而第 5 处行」（归属校验 vs 调用 uid），并记下模式常量的实测值
  （`WAIT_FOR_FINISH=2`，原规范记成 `1`）。实施期待定项**清零**。
- **2026-10-07 第 6 处登记（letterbox 调查收尾）**：运行期反射清单 **5 → 6 处**，新增
  `VirtualDisplayConfig.Builder#setIgnoreActivitySizeRestrictions(boolean)`。它是"活动的尺寸限制"
  （= 兼容窗口 / letterbox）的**唯一解除入口**：请求 `ALWAYS_UNLOCKED` 时平台一定连带
  `OWN_DISPLAY_GROUP`，带显示组的屏会照常采纳应用的固定方向请求 ⇒ 锁定方向的 App 被裁成 0.82 比例。
  应用后 **1920×1800 实测满屏** `Rect(0,0-1920,1800)` ✓（此前为 77% ✗），解锁位照旧生效 ✓。
  取得依据：同类工具 AndroMeld 反编译（它同路由直调）+ 本机 `framework.jar` 存在性 + api-versions 缺位。
  **该处可降级**（拿不到只告警一次）⇒ 它是清单里第一处**非 fatal** 的反射，§4 纪律 2 对它不适用。
- **2026-10-07 第 7 处登记（VDM 实施）**：运行期反射清单 **6 → 7 处**，新增**虚拟设备域**
  （`ServiceManager` → `IVirtualDeviceManager` → `VirtualDeviceManager` 隐藏构造器 → `VirtualDeviceParams$Builder`
  → `createVirtualDevice` → `VirtualDevice.createVirtualDisplay`，外加 `ContextImpl.mAttributionSource` 字段）。
  **动因**：把"锁屏/息屏不接管"与"铺满"从互斥变为可兼得 —— 默认设备上的屏若请求 `ALWAYS_UNLOCKED`，
  平台会**丢弃**该位（除非它在非默认显示组，而非默认组又会照常采纳应用的固定方向请求 ⇒ 裁切）；
  虚拟设备的屏天然在设备组里，解锁位被接受，铺满交给第 6 处（16 起）或尺寸规则（14/15）。
  同时把第 6 处的"为什么躲不掉"改写为**修正后的因果链**（原文"请求解锁位 ⇒ 平台强塞 `OWN_DISPLAY_GROUP`"
  是误读：那位一直是 `DisplayFlags` 自己派生的，见 [`plans/2026-10-07-vdm-mechanism-corrected.md`](../../plans/2026-10-07-vdm-mechanism-corrected.md) §3）。
  双端实测：真机 16 建屏回读通过（无 `FLAG_OWN_DISPLAY_GROUP`）、1920×1800 满屏、锁屏与息屏两档照常出画、
  PC 端到端出画；AVD 15 同路径建屏成功（尺寸开关缺席 ⇒ 兜底生效）。
- **2026-10-07 裁切机制纠正（源码级复核）**：§3 第 6 处一行与 §4 的"降级"表述改写 ——
  裁切是 **fixed-orientation letterbox**（三条触发条件，见 [`display-letterbox.md`](display-letterbox.md)），
  **不是**"非默认显示组 ⇒ 照常采纳固定方向请求"这条因果；被裁比例也**不是**固定的 1.22
  （AOSP 15 默认走**分屏比例**：横屏屏 `宽 = (屏宽 − 30)/2`；真机 16 才是配置值 1.22）。
  "尺寸规则（14/15）"的说法作废 ⇒ 改为"**屏方向 = App 请求方向**"。
  本文件正文（§3 第 6 处、§4）已同步；上面的历史条目保留原样，以本条为准。
- **2026-10-08 产品决定：放弃"铺满"**：§3 第 6 处的正文由"14/15 只能靠『屏方向 = App 请求方向』规避"
  改为"**14/15 没有它 ⇒ 这类 App 会被裁（不为此另做动作）**"；§4 的"降级"表述同步改为"回到平台默认行为"。
  第 7 处一行去掉"与铺满不再互斥"的框架（VDM 的理由就是 `ALWAYS_UNLOCKED` 本身）。
  理由与留档见 [`agent/display-letterbox.md`](display-letterbox.md) §6、[`agent/display-changelog.md`](display-changelog.md)。
