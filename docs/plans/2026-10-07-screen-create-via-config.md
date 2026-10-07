# `:agent`：改用 `VirtualDisplayConfig` 建屏（试图兼得"解锁位"与"铺满"）

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 定义的**实现方案文档**。
> 范围：**只在建屏调用路径上做等效替换**，flags 组合规则与参数模型不变。
> 写完即冻结；实施偏差另建文档。

**状态**：**草案（待批准）**（2026-10-07）。

---

## 1. 背景与证据（全部实机）

- **锁屏/休眠对照**（同一时刻、同一手机、两块虚拟屏并存）：
  我们的屏**全黑** ✗；AndroMeld 的屏**照常显示 App** ✓
  ⇒ **`ALWAYS_UNLOCKED` 就是"锁屏/休眠不影响该屏"的那个位**。
- 但经**旧重载** `createVirtualDisplay(name, w, h, dpi, surface, flags)` 请求该位时，平台**强制带上
  `OWN_DISPLAY_GROUP`**（第 3 轮回读：`期望 0xe88 实际 0xc88`）⇒ 该屏 `ignoreOrientationRequest`
  **恒为 false 且设不上去** ⇒ 锁定方向的 App 被裁成 **0.82 比例**兼容窗口（"铺满"丢失）。
- 实测 AndroMeld 的屏：**有 `ALWAYS_UNLOCKED`、无 `OWN_DISPLAY_GROUP`、`ignoreOrientationRequest=true`**
  ⇒ **这个组合是可达的**，只是不在旧重载上。

## 2. 关键发现：公开 API 里就有 AndroMeld 的那条路

对本机 SDK（`android-37.0`）做字节码检查：

```
public final class android.hardware.display.VirtualDisplayConfig implements Parcelable
public final class VirtualDisplayConfig$Builder { setFlags(int) · setSurface(Surface) · … · build() }
public VirtualDisplay createVirtualDisplay(VirtualDisplayConfig)
public VirtualDisplay createVirtualDisplay(VirtualDisplayConfig, Handler, VirtualDisplay$Callback)
```

⇒ **走同一路线不需要新增反射**：flag **数值**仍由既有的 `wrappers/VirtualDisplayFlags` 提供
（那是**既有**反射点，运行期反射总数仍为 **5 处**）。

## 3. 假设与验证方式

**假设**：显示组是**旧重载路径**的连带行为；用 `VirtualDisplayConfig` 路径 + `ALWAYS_UNLOCKED`
**不再**被强塞 `OWN_DISPLAY_GROUP`。

| 步 | 动作 | 判据 |
|---|---|---|
| 1 | 解锁位缺省恢复 `true`（当前为 `false`），建屏改走新路径 | 编译通过、单测全绿 |
| 2 | 建屏后**回读 flags** | 有 `FLAG_OWN_DISPLAY_GROUP` ⇒ **假设否**（回滚到现状）；**无** ⇒ 假设成立 |
| 3 | 假设成立时补验 | ① 1920×1800 下 B 站**满屏**（`Rect(0,0-1920,1800)`）② **锁屏/休眠后虚拟屏仍显示 App**（与 AndroMeld 逐字对照） |

## 4. 影响面

| 区域 | 变化 |
|---|---|
| `display/Screen.kt` | 建屏调用点改为 `VirtualDisplayConfig`（装配参数等价） |
| `display/DisplayFlags.kt` | 解锁位缺省 `false → true`（若步骤 2 通过） |
| `wrappers/VirtualDisplayFlags.kt` | **不变**（数值来源不变）；如需 Builder 装配入口则在 `wrappers/` 内加一个薄包装 |
| 单测 | `DisplayFlagsTest` 的缺省断言随缺省值回调 |
| 文档 | `display-flags.md` §3/§4 的"代价"段、`display-changelog.md`、调查文档 §5 |

## 5. 风险与回滚

| 风险 | 应对 |
|---|---|
| `VirtualDisplayConfig` 在**运行设备（Android 16）**上未必同样公开 | 编译期用 SDK 37 ✓；运行期**捕获缺失并回退旧重载**（两级路径，回退时解锁位缺省仍为 `false`，即今天的状态） |
| 新路径的 `uniqueId` / `setSurface` 语义与旧重载不同名 | 旧重载内部本就是构造 `VirtualDisplayConfig`，语义等价；差异以实测为准 |
| 回滚 | 恢复旧重载 + 解锁位缺省 `false`（今天已验证的组合） |

---

## 6. 变更记录

- **2026-10-07 实施并实测 —— 假设被证伪，已回滚**：
  按 §3 步骤 1 改走 `VirtualDisplayConfig` 后建屏，回读结果是
  **`期望 0xe80 实际 0xc80`**（差 `0x200` = `FLAG_OWN_DISPLAY_GROUP`）——
  **平台在两条建屏路径上都会给带 `ALWAYS_UNLOCKED` 的屏强塞显示组** ✗。
  （该结果由 agent 自己的 `screenVerify` 回读校验当场拦下，未产生半成品会话 —— 守卫有效 ✓）
  按 §3 步骤 2 的约定**回滚**：建屏恢复旧重载、解锁位缺省恢复 `false`、
  `DisplayFlags` 恢复"解锁位 ⇒ 派生显示组"（期望值必须跟平台一致，否则回读校验必然失败）。
  `Screen.kt` / `DisplayFlags.kt` 留下记录性注释指向本文件。
  **结论**：公开 API 下，"解锁位（锁屏/休眠不接管该屏）"与"铺满（屏能忽略应用固定方向请求）"
  **只能二选一**；AndroMeld 同时拿到两者，**其手段仍未查明**（它确实用了
  `VirtualDisplayConfig`，但仅此不足以解释 ⇒ 还有别的差异。候选：它屏上的
  `FusionSecondaryHomeActivity` / 任务组织方式，或建屏时其它我们尚未发现的设置）。

- **2026-10-07 第 2 轮（补齐 AndroMeld 的 flag 差异）—— 同样无效，已回滚**：
  解锁位 + 显示组 + `PRESENTATION` + `ROTATES_WITH_CONTENT` 一起请求。实测：
  - 两处实验位**确实设上**（设备侧 flags 可见 ✓），但 **`FLAG_OWN_DISPLAY_GROUP` 依然存在** ✗
    （建屏回读通过 ⇒ 平台照样塞）；
  - B站 **仍被裁**：`sw676dp w676dp h825dp 192dpi lrg port`、`mBounds=Rect(207,0-1018,990)` ✗；
  - **更锋利的因果（本轮新得）**：**只要屏在独立显示组里，它就照常采纳应用的固定方向请求** ——
    此轮 `wm get-ignore-orientation-request` 明确报 `true`，屏**照样**裁 ✗
    ⇒ 该设置在有显示组时**形同虚设**（此前被 `get` 的返回值误导过，现纠正）；
  - 解锁位**确实有效**：锁屏/按电源键后该屏**照常显示 App**（截图实证 ✓）——
    即它买到的是"锁屏不影响该屏"，代价是上面的裁切 ⇒ **不划算**，产品仍选"铺满"。
  ⇒ **两轮、4 种组合全部无法拿到"解锁位 + 无显示组"**。若继续追，只剩两条路：
  **隐藏建屏路径**（`DisplayManagerGlobal` + 自建 `VirtualDisplayConfig`，需新增反射）或**放弃**。

- **2026-10-07 定案（用户取向：主屏锁屏时虚拟屏要可用）**：**恢复"解锁位缺省开"**。
  同时纠正上一轮"二选一"的**适用范围**：冲突只在**横屏尺寸**上成立 —— 兼容窗口的宽度被**屏宽**框住，
  **屏的 高÷宽 ≥ 1.22 时它依然是满屏**。实测（1080×1920 @560，解锁位 + 显示组都在）：
  B站 窗口 `Rect(0,0-1080,1920)` ✓，配置 `sw309dp w309dp h549dp 560dpi nrml long`。
  ⇒ **最终产品组合 = 解锁位开 + 尺寸选够竖的**（1080×1920 / 1440×3200 …）。

- **2026-10-07 收尾（反编译 AndroMeld + 第 6 处反射）—— 目标达成**：
  用 jadx 反编译同类工具（`com.catchingnow.andfiles.helper`，21.6 MB；它自带 `libadb.so`，
  shell 侧由 `MirrorServerMain` 跑在 `app_process` 上）后查明它同时具备两条路：
  ① **Android 16 起优先走 VDM**：`android.companion.virtual.VirtualDeviceManager` +
  `VirtualDeviceParams` + `createVirtualDevice` + `createVirtualDisplay(virtualDevice, …)`，**全反射**；
  虚拟设备是"另一台设备" ⇒ 天然绕开显示组（这就是它"有解锁位、无显示组"的根因）。
  ② **回退路径**走公开 `createVirtualDisplay(...)`，但会在 `VirtualDisplayConfig` 上**直调隐藏的**
  `setIgnoreActivitySizeRestrictions(true)`（它编译时用了"补全隐藏 API 的 jar" —— 我们否掉的那条路；
  但我们**只需要反射这 1 个方法**，不必走 jar 那条路）。
  ⇒ 实务取最便宜的一招：建屏路径改走公开 `VirtualDisplayConfig` 装配 + **反射这一个方法**
  （[`code-hiddenapi.md`](../spec/agent/code-hiddenapi.md) §3 第 6 处；**Android 16 起才有、可降级**）。
  实测：**1920×1800 满屏** `Rect(0,0-1920,1800)` ✓（此前 77% ✗）、`sizeRestrictionsIgnored=true` ✓、
  解锁位照旧生效 ✓ ⇒ **"主屏锁屏可用"与"任意尺寸铺满"兼得**，尺寸不再受 1.22 约束。
  Android 14/15 上自动降级回"尺寸够竖"的退路。

- **2026-10-07 Android 15 实机验证（x86_64 AVD，SDK 35）**：
  - **API 面**（拉该机 `framework.jar` 逐 dex 扫字符串）：`VirtualDisplayConfig` ✓、`VirtualDeviceManager` ✓、
    `createVirtualDevice` ✓ 均**存在**；**`setIgnoreActivitySizeRestrictions` 0 命中 —— 不存在** ✓
    ⇒ "Android 16 起才有"由推断升为**实测**。
  - **降级路径**：把 agent 推到该 AVD 运行，日志逐字如设计 ——
    `本机没有 setIgnoreActivitySizeRestrictions（Android 16 起才有）⇒ 兼容窗口改按"尺寸够竖"规避` ✓
    （不崩溃、不阻断启动）。
  - 同一次运行还顺带验证了另一道闸门：`[SUBSYSTEM/noHwEncoder] 本机没有可用的硬编编码器（绝不退软编）`
    ⇒ 退出码 **2** ✓（AVD 无硬编，故完整链路无法在模拟器上跑 —— 这是既定约束，不是缺陷）。
  - [未验证] Android 15/14 上"解锁位 ⇒ 强制显示组 ⇒ 裁切"这条因果**是否同样成立**：
    需要一台**有硬编**的 14/15 设备才能测（模拟器过不了能力闸门 ⇒ 不为此加调试开关，F8 已定不加）。
  - **探针实测（同日在同一台 AVD 上补测）**：写了一个**只建屏、不碰编码器**的探针（复用 agent 的
    bootstrap 与同一套 flags 组合；输出面用 `ImageReader`，并把帧落盘 —— 模拟器的 `screencap -d`
    抓不到虚拟屏）。结论：
    ① **Android 15 同样被强塞 `FLAG_OWN_DISPLAY_GROUP`**：回读 `0xf80`（含 `0x100`）、设备侧 flags
    亦列出 ⇒ **与 16 一致** ⇒ "解锁位与铺满冲突"的老问题在 14/15 上同样存在，降级退路（尺寸 ≥ 1.22）确实必要；
    ② 该屏上起 AOSP 自带 App（设置 / 相机 / 时钟）⇒ 窗口均 `frame=[0,0][1920,1800]` **满屏** ✓，
    且该屏**确实在出帧**（探针 ImageReader 计数 41）⇒ 建屏路径在 15 上可用 ✓；
    ③ `sizeRestrictionsIgnored=false` ✓ —— 隐藏开关在 15 上缺席，**降级路径按设计生效**（不崩、照常建屏）。
    ④ **[仍未测] 该屏的锁屏行为**：AVD 拒绝进入休眠（`KEYCODE_SLEEP` 后仍 `mWakefulness=Awake`、
    `isKeyguardShowing=false`）⇒ 模拟器上无法验证；该性质在真机（Android 16）上**已两次截图证实** ✓。
  - 探针是一次性工具，**测完已从仓库删除**（无残留）。
