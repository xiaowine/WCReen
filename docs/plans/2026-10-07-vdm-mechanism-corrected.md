# 修正：VDM 屏的解耦与「铺满」的真实开关

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 定义的**实现方案文档**（实测修正记录）。
> 范围：纠正 [`2026-10-07-single-path-virtual-device.md`](2026-10-07-single-path-virtual-device.md) §3 的机制结论，
> 并给出**修正后的多版本矩阵**。写完即冻结；后续结论另建文档。

**状态**：**已实测**（2026-10-07；真机 Android 16 + AVD Android 15；一次性探针）。

**取代关系**（旧文档按惯例冻结不改）：

- [`2026-10-07-single-path-virtual-device.md`](2026-10-07-single-path-virtual-device.md) §3 的
  "屏建在设备上 ⇒ 平台不再强塞 `OWN_DISPLAY_GROUP` ⇒ 忽略方向请求生效 ⇒ 任意尺寸铺满" —— **被证伪**
  （VDM 屏 `ignoreOrientationRequest=false`，锁定方向的 App 照样被裁 1.22）；
  其 §1 目标 ④「无版本分支 / 无尺寸规避」—— **不可达**（14/15 没有尺寸开关）；
- [`2026-10-07-letterbox-investigation.md`](2026-10-07-letterbox-investigation.md) §4.1 与
  [`agent/display-flags.md`](../spec/agent/display-flags.md) §4 的因果链
  "请求 `ALWAYS_UNLOCKED` ⇒ 平台强塞 `OWN_DISPLAY_GROUP`" —— **误读**（见 §3）。

---

## 1. 一句话结论

**VDM 买到的是「锁屏/息屏解耦」与「拉起的正门」，不是「铺满」。**
铺满的真正开关是 `VirtualDisplayConfig.Builder#setIgnoreActivitySizeRestrictions(true)`
（**Android 16 起**；14/15 只能靠"尺寸 高÷宽 ≥ 1.22"规避）。

## 2. 实测证据（本轮，全部设备侧实读）

| # | 场景 | 实测 |
|---|---|---|
| 1 | 真机 16 · VDM · 1920×1800 · 无尺寸开关 | 屏 `ignoreOrientationRequest=false`；App 窗口 `frame=[223,0][1698,1800]` ⇒ **裁切 1.22** |
| 2 | 真机 16 · 同一 config + 反射 `setIgnoreActivitySizeRestrictions(true)` | 反射**成功**；`frame=[0,0][1920,1800]`、`letterBoxed=false` ⇒ **满屏** ✓ |
| 3 | 真机 16 · **息屏档**（`mWakefulness=Dozing`、物理屏 `mScreenState=OFF`） | 虚拟屏**照常渲染 App**（像素取证）✓ |
| 4 | 真机 16 · **锁屏档**（`mKeyguardShowing=true`） | 虚拟屏**照常渲染 App**（像素取证）✓；物理屏为黑 + 锁屏指示 |
| 5 | AVD 15 · VDM · 1080×1920（无尺寸开关，靠尺寸规则） | `frame=[0,0][1080,1920]` ⇒ **满屏** ✓ |
| 6 | 两端 · `launchPendingIntent(int, PendingIntent, Executor, IntConsumer)` | 调用成功且 **App 真的上屏**（该 display 上出现 task + window + 出帧）✓ |
| 7 | 真机 16 · `wm set-ignore-orientation-request -d <屏> true` | **静默无效**（值仍 `false`；连屏 0 也改不动）⇒ 不可作手段 |

回读位（两端一致）：`0xe80 = TRUSTED｜ALWAYS_UNLOCKED｜TOUCH_FEEDBACK_DISABLED｜OWN_FOCUS`，
**无 `OWN_DISPLAY_GROUP`**（屏的显示组 = 虚拟设备的组）。

## 3. AOSP 规则（`android-16.0.0_r2` 逐句核对）

`DisplayManagerService.createVirtualDisplayInternal`：

- **丢弃**解锁位：`ALWAYS_UNLOCKED` 且 `OWN_DISPLAY_GROUP`、`DEVICE_DISPLAY_GROUP` **都缺** ⇒
  日志 "Always unlocked displays cannot be in the default display group, hence ignoring flag
  VIRTUAL_DISPLAY_FLAG_ALWAYS_UNLOCKED." 并**清掉该位** —— 平台**不会**替调用方补显示组。
- **VDM 自动进设备组**：未请求 `OWN_DISPLAY_GROUP`、非自动镜像、带 `TRUSTED`、且 `virtualDevice != null`
  ⇒ 平台**加上** `DEVICE_DISPLAY_GROUP`（屏进该设备的显示组）。
- 权限门：`ADD_TRUSTED_DISPLAY`（建可信屏 / 非默认组）、`ADD_ALWAYS_UNLOCKED_DISPLAY`（解锁位）。

**由此定位旧文档的误读**：早期回读出现的 `FLAG_OWN_DISPLAY_GROUP` 来自 **`agent/display/DisplayFlags.kt`
自身的派生规则**（`ALWAYS_UNLOCKED ⇒ OWN_DISPLAY_GROUP`），不是平台强塞；letterbox 调查第 3 轮的
"期望 `0xe88` 实际 `0xc88`" 实际是 **`0x200`（解锁位）被丢弃**，而非"补上显示组"。

**裁切根因（修正后）**：屏是否属于**非默认显示组** ⇒ 决定该屏的 `ignoreOrientationRequest`
（非默认组 ⇒ `false`）⇒ 应用的固定方向请求被尊重 ⇒ 1.22 兼容窗。
而解锁位**只对非默认组的屏有效** ⇒ ≤ Android 15 上「解锁位」与「任意尺寸铺满」**天然互斥**。

`VirtualDisplayConfig.Builder#setIgnoreActivitySizeRestrictions(boolean)`（javadoc）：
"ignores fixed orientation, aspect ratio and resizability of apps"，**要求屏带 `TRUSTED`**；
`@FlaggedApi(FLAG_VDM_FORCE_APP_UNIVERSAL_RESIZABLE_API)` + 平台 flag，**Android 16 起才有**
（15 `framework.jar` 0 命中）。它**绕开**上面那条互斥 —— 不论屏在哪个显示组。

## 4. 修正后的多版本矩阵

| 版本 | 锁屏/息屏解耦 | 铺满 | 依据 |
|---|---|---|---|
| 14 / 15 | `setLockState(LOCK_STATE_ALWAYS_UNLOCKED)` + 屏 `ALWAYS_UNLOCKED` ✓ | **只能**靠尺寸：屏 高÷宽 ≥ 1.22（1080×1920 ✓） | 14 源码面核对；15 AVD 实测（§2 #5） |
| 16+ | 同上 ✓（像素取证） | 同一 config 上 `setIgnoreActivitySizeRestrictions(true)` ⇒ **任意尺寸** ✓ | §2 #2/#3/#4 |
| 存在性 | `VirtualDeviceManager` API 34 起；`setLockState` / `VirtualDevice.createVirtualDisplay(VirtualDisplayConfig…)` / `launchPendingIntent` 在 14 源码中均存在 | `setIgnoreActivitySizeRestrictions` 16 起 | AOSP 各分之源码 + 两端运行期 |

**拉起的正门**（与铺满无关、但同属该 API 族）：
`VirtualDevice.launchPendingIntent(displayId, PendingIntent, Executor, IntConsumer)` —— 由设备自身发起，
不经调用方的 `--display` 校验（真机上 `am start --display` 被 WM 拒的那条路由此绕开）。

## 5. 复跑配方（自持）

探针：`agent/src/main/kotlin/com/xiaowine/wcreen/agent/VdmDump.kt`（**一次性工具，端到端验收后删除**）。

```
CLASSPATH=/data/local/tmp/wcreen-agent.jar app_process /system/bin \
  com.xiaowine.wcreen.agent.VdmDumpKt <关联ID> <保持秒> <宽> <高> <密度> <包名> <组件名> \
      <unlocked|default> <帧落盘前缀> <屏解锁位0|1> <尺寸开关0|1>
```

判据：`dumpsys window windows` 里该 App 的 `Frames: frame=[0,0][宽,高]`（满屏）与
`dumpsys display` 里该屏的 flags；锁屏档另用 `dumpsys power | grep mWakefulness` 与
`dumpsys activity | grep mKeyguardShowing` 确认档位，`screencap -d <SFid>`（SFid 取
`dumpsys SurfaceFlinger --display-id`）取原始帧。

## 6. 未决

1. **PC 端到端出画**（本方案链路的最后一环）未验；
2. **Android 14 真机**无设备（仅源码面核对）；
3. **Android 15 的锁屏档**在 AVD 上不可验（模拟器拒绝休眠）；
4. 多路（多屏）时"一个设备 vs 每路一个设备"仍待定（原方案已列）。

## 7. 变更记录

- **2026-10-07 修正**：双端实测把"VDM ⇒ 自动铺满"证伪，定位真正开关为 `setIgnoreActivitySizeRestrictions`
  （16 起）与"尺寸 ≥1.22"（14/15）；据 AOSP 源码纠正"平台强塞 `OWN_DISPLAY_GROUP`"的误读；
  补双档（锁屏 / 息屏）像素取证与 `launchPendingIntent` 上屏实证。
