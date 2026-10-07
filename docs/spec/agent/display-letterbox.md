# `:agent` · 虚拟屏上的裁切（letterbox）与"铺满"留档（F1）

> 本文件承接 [`agent/display-flags.md`](display-flags.md) §4 的**细节**（父文件只留摘要 + 链接）。
> **2026-10-07 源码级复核**（AOSP 15 源码 + 模拟器 A15 实测 + 真机 16 复核）：旧结论
> 「14/15 只能靠 **屏的 高÷宽 ≥ 1.22**」与「`ignoreOrientationRequest=false` ⇒ 被裁」**作废**，见 §5。

---

## 1. 现象与判据来源

锁定方向的 App 在我们的虚拟屏上被裁成一条**居中窗口**、四周留黑边。
`dumpsys activity activities`（该 ActivityRecord 段）**直接给出原因与结果**，不需要猜：

```text
resizeMode=RESIZE_MODE_UNRESIZEABLE
supportsSizeChanges=SIZE_CHANGES_UNSUPPORTED_METADATA
overrideOrientation=SCREEN_ORIENTATION_PORTRAIT
areBoundsLetterboxed=true
  letterboxReason=FIXED_ORIENTATION      ← 就是它（不是 ASPECT_RATIO / SIZE_COMPAT_MODE / DISPLAY_CUTOUT）
  activityAspectRatio=1.1428572
```

`dumpsys activity containers` 同时给出**裁在哪一层**：`Task` 仍是全屏 `[0,0][1920,1080]`，
只有 `ActivityRecord` 缩成 `[488,0][1433,1080]` ⇒ 是**活动级** letterbox，不是 Task/屏级。

---

## 2. 触发条件（A15/14：三条**同时**成立才裁）

出处：AOSP 15 `ActivityRecord.resolveFixedOrientationConfiguration()`（`android-15.0.0_r1` L9023–9158；
本文所有行号均指该 tag）。

| # | 条件 | 判据（源码） |
|---|---|---|
| 1 | App **不可缩放且固定方向** | `shouldCreateCompatDisplayInsets()`（L8406）= `!isResizeable() && (isFixedOrientation() \|\| hasFixedAspectRatio())` |
| 2 | **请求方向 ≠ 父（屏）方向** | 父方向按 bounds 判（L9046 / L8982）：`高 ≥ 宽 ⇒ PORTRAIT`，否则 `LANDSCAPE` |
| 3 | **屏不会为它旋转** | `DisplayContent.handlesOrientationChangeFromDescendant()`（L1715）= `!shouldIgnoreOrientationRequest(orientation) && !isFixedToUserRotation()` |

**我们屏上的取值**（VDM 建的屏，`dumpsys window displays` 实测）：**`mFixedToUserRotation=true`**
⇒ 条件 3 **恒成立**（屏永不为 App 旋转）⇒ 只要条件 1、2 成立就裁。

⚠️ 条件 1 涉及 `resizeableActivity`，但**不是**"可缩放就不裁"：可缩放只影响 §3 用哪一支比例，
**固定方向请求本身**就会触发裁切（真机 16 上可缩放的 B 站同样被裁成 0.819 比例，见 §4）。

---

## 3. 裁成多大

两步（L9101 + `applyAspectRatio()` L9665–9782）：

1. 先把容器**压成正方形**（`宽 = 屏高`，居中）；
2. 再按「固定方向 letterbox 宽高比」缩宽（横屏屏 ⇒ 缩宽；竖屏屏 ⇒ 缩高）。

该比例取哪一支由**设备配置**决定（`LetterboxUiController` L952–958 / L995–1005）：

| 设备配置 | 用的比例 | 结果 |
|---|---|---|
| `isSplitScreenAspectRatioForUnresizableAppsEnabled = true`（**AOSP 15 默认**） | **分屏比例**（L1068–1088）= 屏 app bounds 短边减去 `dividerSize` 后取一半 | 横屏：**宽 = (屏宽 − dividerSize)/2、高 = 屏高** |
| 该开关 `false` | `config_fixedOrientationLetterboxAspectRatio`（AOSP 默认 4:3） | 比例**与屏尺寸无关**：宽 = 屏高 ÷ 该值 |

`dividerSize = docked_stack_divider_thickness − 2 × docked_stack_divider_insets`。
本仓库环境实测：`48dp − 2×19dp = 10dp`（从设备 `framework-res.apk` 经 `aapt2 dump resources` 读出）
⇒ **@480dpi = 30px**。

> ⚠️ `dumpsys` 里的 `fixedOrientationLetterboxAspectRatio=1.333` 是**配置值**，在模拟器上**没被采用**
> （走的是分屏比例）—— 判据要看 `activityAspectRatio`（结果）与 §3 的配置开关，别只看这一行。

---

## 4. 实测矩阵（两端）

**模拟器 A15 / SDK 35**（软编，测试 App `com.xiaowine.sizetest`：`resizeableActivity=false` +
`screenOrientation=portrait`、`targetSdk=32`）；屏 `mFixedToUserRotation=true`、
`isSplitScreenAspectRatioForUnresizableAppsEnabled=true` ⇒ 用**分屏比例**：

| 屏 | `高 ≥ 宽`? | App 窗口（`dumpsys` 实测） | 结论 |
|---|---|---|---|
| 1080×1920 | 是 | `[0,0][1080,1920]` | **满屏** ✓（`areBoundsLetterboxed=false`） |
| 1080×1080 | 是（`≥` 取等） | `[0,0][1080,1080]` | **满屏** ✓ |
| 1920×1080 | 否 | `[488,0][1433,1080]` = 945×1080 | 裁切 |
| 1600×900 | 否 | 785×900 | 裁切 |
| 1280×720 | 否 | 625×720 | 裁切 |
| 1024×768 | 否 | 497×768 | 裁切 |

四个横屏尺寸**全部**等于 `(屏宽 − 30)/2 × 屏高` ✓。
注意 `1024×768` 的 `高÷宽 = 1.333 ≥ 1.22` **照样被裁** ⇒ 数值比例不是判据。

**真机 Android 16 / SDK 36**：`wm get-letterbox-style` ⇒ `Aspect ratio: 1.22`、
`Is using split screen aspect ratio as aspect ratio for unresizable apps: false` ⇒ 用**配置比例 1.22**，
窗口比例恒为 `1/1.22 ≈ 0.819`（**与屏尺寸、密度无关**）；加 `setIgnoreActivitySizeRestrictions(true)`
后 **1920×1800 满屏** `frame=[0,0][1920,1800]` ✓。

---

## 5. 旧结论错在哪（保留记录，勿再引用旧表述）

- 「14/15 只能靠 **屏的 高÷宽 ≥ 1.22** 才铺满」—— 把**某一台设备的配置值**
  （`config_fixedOrientationLetterboxAspectRatio = 1.22`）当成了平台规则。反例：模拟器 `1024×768`
  （1.333 ≥ 1.22）被裁成 497×768；同一条规则也无法解释模拟器上的分屏比例分支。
- 「屏 `ignoreOrientationRequest=false` ⇒ 照常采纳应用的固定方向请求 ⇒ 被裁」—— **因果反了**：
  `ignoreOrientationRequest=false` 只说明屏**没被要求忽略**请求；真正让请求落不了地的是
  **`mFixedToUserRotation=true`**（VDM 建的屏固定旋转，无法靠旋转满足）。
- 「解锁位 / `OWN_DISPLAY_GROUP` 与裁切有因果关系」—— **无**。裁切只看 §2 三条；解锁位只影响
  "主屏锁屏/息屏是否接管该屏"（[`agent/display-lifecycle.md`](display-lifecycle.md)）。

---

## 6. 可选做法与产品取舍

**产品决定（2026-10-08）：放弃"铺满"这个目标** —— 14/15 上不再做任何"让 App 铺满"的额外动作，
App 被裁（fixed-orientation letterbox / size-compat）按平台行为接受。本节把**实测过的可选做法**留档，
供将来重新评估；**当前均未启用**（A 曾实现，已回退，见
[`plans/2026-10-08-screen-rotatable-implementation.md`](../../plans/2026-10-08-screen-rotatable-implementation.md)）。

**16+ 的尺寸开关仍然生效**：`setIgnoreActivitySizeRestrictions(true)` 在 Android 16 起可用
（[`agent/code-hiddenapi.md`](code-hiddenapi.md) §3 第 6 处），实测 1920×1800 满屏；它**不需要**本节其余任何机制。

**可选做法**（模拟器 A15 / SDK 35 实测，屏 1920×1080，竖屏锁定 App）：

| 路 | 做法 | 实测 | 代价 |
|---|---|---|---|
| **A. 让屏可旋转** | 建屏后执行 `wm fixed-to-user-rotation -d <id> disabled`（= `cmd window fixed-to-user-rotation -d <id> disabled`，**shell 权限即可**） | 屏被转到 App 方向：`rot=3 cur=1080x1920`、ActivityRecord `[0,0][1080,1920]`、`areBoundsLetterboxed=false` ✓（**VDM 屏实测同样成立，解锁位不受影响**） | **屏逻辑方向翻转**：帧缓冲仍是 1920×1080，但内容转 90° ⇒ PC 侧要按 rotation 旋转画面；且**横屏帧装竖屏内容会浪费像素**（同码率画质更差）；不可缩放 App 首拉需重启一次 |
| **B. 按 App 方向建屏** | 竖屏 App ⇒ `高 ≥ 宽` 的屏（**含方屏**，判据取 `≥`）；横屏 App ⇒ `宽 > 高` | 1080×1080 / 1080×1920 **满屏** ✓（不旋转、内容正立、帧即内容） | 尺寸不能任意（选了横屏尺寸配竖屏 App 就会裁）；App 运行中改方向会裁 |
| **C. App 无方向请求** | 目标 App `screenOrientation=unspecified` | 不裁 | 取决于目标 App，不可控 |

> **A 的实施与回退**（2026-10-08）：曾按 [`plans/2026-10-07-screen-rotatable.md`](../../plans/2026-10-07-screen-rotatable.md)
> 实现（agent 设 `disabled` + `DisplayListener` 检测方向 + `screenRotation` 主动上报 + PC 按方向旋转渲染），
> 双端实测通过后**因"维护面不值"回退**（含 `applied.rotation` 与 `screenRotation` 一并撤销）。
> 若要重新启用，按该方案 + 实施记录即可。

**死路**（同批实测；这是"之前的代码"——关解锁位 / 默认显示组 / 置 `ignoreOrientationRequest`——的招式，**在 14/15 上全部无效**）：

| 组合（A15，屏 1920×1080，竖屏锁定 App） | 结果 |
|---|---|
| VDM 屏 + `ignoreOrientationRequest=false` | 945×1080（`letterboxReason=FIXED_ORIENTATION`）✗ |
| VDM 屏 + `ignoreOrientationRequest=true`（`wm set` 成功并回读确认） | 945×1080 ✗ |
| **旧路径**屏（`DisplayManager.createVirtualDisplay`，非 VDM）+ **默认显示组**（`displayGroupId 0`）+ `ignore=false` | 945×1080 ✗ |
| 旧路径屏 + 默认显示组 + `ignore=true` | 945×1080 ✗ |

⇒ **`ignoreOrientationRequest` 在 14/15 上治不了这条 letterbox**：它只决定"屏要不要理会 App 的方向请求"，
而屏**不能旋转**时系统只能裁（§2 条件 3）。14/15 上该标志在虚拟屏上**默认就是 `false`**
（与 16 不同）；round 4 那套"关解锁位 ⇒ 默认组 ⇒ `ignore=true` ⇒ 铺满"**只在 Android 16 上成立**，
**不可移植到 14/15**（16 侧另有一条把 App 方向请求丢掉的处理，属该版本的差异，未逐行追）。

**16+**：`setIgnoreActivitySizeRestrictions(true)`（反射，[`agent/code-hiddenapi.md`](code-hiddenapi.md) §3
第 6 处）可**无视方向** ⇒ 任意尺寸都能铺满（1920×1800 实测 ✓，见 §4）。

---

## 7. 取证方法（可复现）

- **现象与原因**：`adb shell dumpsys activity activities`（看 `letterboxReason` / `activityAspectRatio`）；
  `adb shell dumpsys activity containers`（看 ActivityRecord 与 Task 的 bounds 差）；
  `adb shell dumpsys window displays`（看 `mFixedToUserRotation`）。
- **配置值**：`adb shell wm get-letterbox-style`；
  `aapt2 dump resources <framework-res.apk>`（读 `docked_stack_divider_thickness` / `docked_stack_divider_insets`）。
- **源码**：AOSP 15 的 `ActivityRecord` / `LetterboxUiController` / `DisplayContent`
  （`android-15.0.0_r1`，cs.android.com 可直接查；设备 build `google/sdk_gphone64_x86_64/emu64xa:15/…`）。
  **不需要反编译 dex**：常量与配置值都能从 `dumpsys` + `aapt2` 在设备上核对。

---

## 8. 变更记录

- **2026-10-07 初版**：源码级复核（AOSP 15 源码 + 模拟器 A15 实测矩阵 + 真机 16 复核），
  纠正「1.22 尺寸规则」与「`ignoreOrientationRequest` 因果」两条旧结论；给出三条触发条件、
  两支比例的公式与双端实测矩阵。旧表述散落在
  [`plans/2026-10-07-letterbox-investigation.md`](../../plans/2026-10-07-letterbox-investigation.md) 与
  [`plans/2026-10-07-vdm-mechanism-corrected.md`](../../plans/2026-10-07-vdm-mechanism-corrected.md)（**方案文件已冻结，不再改**）。
- **2026-10-07 补测（14/15 的可行路与死路）**：§6 改写 —— 用**一次性探针**在模拟器 A15 上把"旧路径 / 默认显示组 /
  `ignoreOrientationRequest`"四组组合跑完，**全部仍被裁**（945×1080，`FIXED_ORIENTATION`）⇒
  round 4 那套（关解锁位 ⇒ 默认组 ⇒ `ignore=true` ⇒ 铺满）**只在 Android 16 上成立，14/15 不可移植**；
  14/15 的可行路实测两条：**按 App 方向建屏**（1080×1080 / 1080×1920 满屏）与
  **让屏可旋转**（`wm fixed-to-user-rotation -d <id> disabled` ⇒ 屏转到 App 方向，VDM 屏上同样成立、
  解锁位不受影响；代价是屏逻辑方向翻转、内容转 90°）。探针用完即删。
