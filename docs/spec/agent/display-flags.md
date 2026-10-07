# `:agent` · 虚拟屏 flags（F1）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/display.md`](display.md)（F1）的**细节**。
> 参数模型、验收判据与生命周期见 [`agent/display.md`](display.md)；硬约束见 [`agent/module.md`](module.md) §3。

---

## 1. 总原则

**一律用常量名拼组合，绝不硬编码数值** —— 这些位全是系统 API（`@hide`），跨版本可能变
（源码里已有 TODO 标注要移除其中两个）。下面按**官方语义**分档。

---

## 2. 必须设置（不可配）

| flag | 官方语义要点 | 为什么必须 |
|---|---|---|
| `TRUSTED` | 可信以显示系统装饰、**且无需用户触摸即可接收输入** | 硬约束 1、2：否则第三方 App 无法在其上启动；也是注入输入的前提 |
| `PUBLIC` | private 屏只有"创建者 UID + 已在该屏上的 Activity"能放窗口 / 枚举 / 交互 | 建屏进程是 shell（uid 2000）；不给 `PUBLIC`，第三方 App 上不去 |
| `OWN_CONTENT_ONLY` | public 屏**无窗口时会自动镜像默认屏**，此位才让它只显示自己的内容（无窗口则空白） | 否则从建屏到拉起 App 之间，PC 看到的是**本机屏镜像** —— 与"独立屏"直接冲突 |
| `SUPPORTS_TOUCH` | 屏可关联触摸板设备 | **产品决定**（**七位中唯一非技术必需**）：保留触摸能力，是否转发事件由 PC 决定（不转发即可）。**已实测**：设了它注入完全正常（§6.4） |
| `OWN_FOCUS` | 屏维持自己的焦点与触摸模式（**需 `TRUSTED`**） | **保留（无害、属官方推荐组合）**。⚠️ **实测：对 `FocusedDisplayId` 与主屏 `mCurrentFocus` 均无影响** —— 原"本机屏不应失焦"的假设**不成立**（§6.4） |
| `TOUCH_FEEDBACK_DISABLED` | 触摸时不播音效、不做触觉反馈 | PC 端点击不该在屏上留反馈 |
| `DESTROY_CONTENT_ON_REMOVAL` | 屏被移除时**销毁**其内容，而不是把内容迁走 | **产品级必需（已实测）**：关掉后**拆屏或进程崩溃都会把内容搬到本机屏并显示**（`visible=true`），直接违反核心承诺。2×2 对照实验见 §6.3 |

---

## 3. 由 PC 直接配置

| flag | 缺省 | PC 关闭时的后果（**必须回报**） |
|---|---|---|
| `SHOULD_SHOW_SYSTEM_DECORATIONS` | **关**（不显示 home / 导航栏 / 壁纸；需 `TRUSTED`） | 屏内出现系统装饰，App 可用高度变小 |
| `ALWAYS_UNLOCKED` | **开**（屏上不显示 keyguard，且主屏锁屏/息屏都不接管本屏） | 关掉后：手机锁屏/息屏时虚拟屏会被 keyguard 接管（或全黑）—— 2026-10-07 双档实测 ✓。**不再有任何连带代价**：屏挂在虚拟设备上（见 §4）。**铺满不是目标**（2026-10-08 产品决定放弃，见 [`display-letterbox.md`](display-letterbox.md) §6） |

---

## 4. 显示组：**不请求**（屏挂在虚拟设备上）

| flag | 是否请求 | 说明 |
|---|---|---|
| `OWN_DISPLAY_GROUP` | **否**（也不再派生） | 屏由**虚拟设备**创建 ⇒ 平台自动把设备屏放进**设备自己的**显示组（AOSP `DisplayManagerService`：未请求 `OWN_DISPLAY_GROUP` + 带 `TRUSTED` + 有虚拟设备 ⇒ 自动加 `DEVICE_DISPLAY_GROUP`）。**解锁位因此被接受**；自己再请求 `OWN_DISPLAY_GROUP` 只会把屏从设备组里拿出来，徒增权限面 |
| `DEVICE_DISPLAY_GROUP` | **否**（平台自动加） | 同上，是平台的连带行为，不是我们的请求项 |

**为什么"必须非默认组"**（AOSP `DisplayManagerService.createVirtualDisplayInternal` 原文语义）：
`ALWAYS_UNLOCKED` 与"留在默认显示组"**不可兼得** —— 平台检测到"要解锁位但两个组位都没有"时会
**直接清掉解锁位**（日志 *"Always unlocked displays cannot be in the default display group, hence
ignoring flag VIRTUAL_DISPLAY_FLAG_ALWAYS_UNLOCKED."*），**不会**替调用方补显示组。
⚠️ 早期文档把这条记成"请求解锁位 ⇒ 平台强塞 `OWN_DISPLAY_GROUP`" —— **是误读**：那位一直是
[`display/DisplayFlags.kt`](../../../agent/src/main/kotlin/com/xiaowine/wcreen/agent/display/DisplayFlags.kt)
自己派生的；letterbox 调查第 3 轮的 `0xc88` 是**解锁位被丢弃**，不是"补上了显示组"。

**由此得到的因果链（修正后）**：

```
屏由虚拟设备创建 ⇒ 该屏 `mFixedToUserRotation=true` ⇒ 永不为 App 的固定方向请求旋转
    ⇒ 14/15 走 **fixed-orientation letterbox**：不可缩放 + 固定方向的 App 被裁
    ⇒ **14/15 没有已启用的"铺满"手段**（产品已放弃该目标，App 被裁按平台行为接受）
    ⇒ **16+** 另有 `setIgnoreActivitySizeRestrictions(true)`（反射，见 code-hiddenapi.md §3 第 6 处）
      ⇒ 任意尺寸不被裁
```

**细节见 [`agent/display-letterbox.md`](display-letterbox.md)** —— 三条触发条件、两支比例
（分屏比例 / `config_fixedOrientationLetterboxAspectRatio`）、双端实测矩阵、旧结论的错因、取证方法，
以及 **14/15 的两条可选做法与代价**（**当前均未启用**：让屏可旋转 / 按 App 方向建屏）与**死路**
（关解锁位、默认组、置 `ignoreOrientationRequest` 在 14/15 上均无效）。

⚠️ **`ignoreOrientationRequest` 既不是开关、也不是原因**：它在虚拟屏上**默认就是 `false`**（15/16 皆然）；
`wm set-ignore-orientation-request -d <屏> true` 在**模拟器 A15 上能设成功**（实测回读 `true`，真机上设不上去），
但**设成 `true` 也照样被裁** —— 真正卡住的是"屏不能旋转"（见 `agent/display-letterbox.md` §6 死路表）。
✅ **16+ 实测**：加上尺寸开关后 **1920×1800 满屏** `frame=[0,0][1920,1800]`、`letterBoxed=false` ✓，
解锁位照旧生效 ✓。

---

## 5. 明确不设置（附原因）

| flag | 原因 |
|---|---|
| `SECURE` | 与"把画面传出去"自相矛盾；创建还需系统保留权限 `CAPTURE_SECURE_VIDEO_OUTPUT`。**副作用记录**：本机 `FLAG_SECURE` 窗口在该屏上会被**涂黑**，这是预期行为 |
| `AUTO_MIRROR` | 与 `OWN_CONTENT_ONLY` 互斥；镜像属后置功能 F9 |
| `CAN_SHOW_WITH_INSECURE_KEYGUARD` | 仅 private 屏可用、与 `PUBLIC` 互斥；官方已标 TODO 要移除 |
| `ALLOWS_CONTENT_MODE_SWITCH` | 允许系统在"扩展 ↔ 镜像"间**动态切换**，与确定性要求冲突；且与 `OWN_CONTENT_ONLY` 互斥 |
| `DEVICE_DISPLAY_GROUP` | 与 `OWN_DISPLAY_GROUP` 是**二选一**的两种"脱离默认组"方式，我们只用后者 |
| `ROTATES_WITH_CONTENT` | 面向"物理朝向固定"的显示设备；我们不是物理屏，方向由尺寸固定 |
| `PRESENTATION` | 会让应用**自动**往该屏投影，行为不可控；不需要 |

---

## 6. 实测结果（已验，见 [`agent/display.md`](display.md) §5）

### 6.1 组合整体成立

用 §2+§3 缺省+§4 派生拼出的组合建屏，**第三方 App（普通 uid）能正常启动并渲染**，
输出面帧计数递增，本机屏不受影响 ⇒ 组合可用。

### 6.2 回读校验（判据 2 的实证）

系统回读的 `Display.getFlags()` **精确等于**我们请求的 4 个高位：

```
FLAG_TRUSTED | FLAG_ALWAYS_UNLOCKED | FLAG_TOUCH_FEEDBACK_DISABLED | FLAG_OWN_FOCUS
```

即：**`FLAG_PRESENTATION`、`FLAG_PRIVATE`、`FLAG_SHOULD_SHOW_SYSTEM_DECORATIONS`、
`FLAG_OWN_DISPLAY_GROUP` 均未置位** ⇒ 「不镜像、公开、装饰关闭、在设备自己的组里」四条意图都被系统如实执行。
（2026-10-07 双端实测：真机 16 与 AVD 15 回读一致为 `0xe80`；`FLAG_OWN_DISPLAY_GROUP` 在**掩码内**，
一旦出现即回读不符 ⇒ 建屏失败 —— 这是"屏没进设备组"的自动报警。）

### 6.3 `DESTROY_CONTENT_ON_REMOVAL` 的对照实验（2×2）

| | 显式 `release()` | 进程崩溃（`halt`，不释放） |
|---|---|---|
| **开**（缺省） | 内容留成**不可见**的僵尸 task，**本机屏干净** ✓ | **同上** ✓ |
| **关** | **内容搬到本机屏并 `visible=true`** ✗ | **同上** ✗ |

⇒ 该 flag **由 system_server 侧生效，与释放路径无关，也不依赖我们的进程活着**。
⇒ 由此**升格为固定必须位**（§2）：它不是"可选偏好"，关掉没有任何合理用途，只有事故。
⇒ 同时它是"不影响本机屏"承诺在**最坏路径**（agent 崩溃 / 被系统杀）上的唯一保障。

### 6.4 已测（F5 起可测）—— **`OWN_FOCUS` 的原假设被推翻**

**实测条件**：往虚拟屏注入触摸，观测 `dumpsys input` 的 `FocusedDisplayId` 与
`dumpsys window displays` 里**每块屏各自的** `mCurrentFocus`（按行号归属，不靠段头猜测）。

| 项 | 实测结果 |
|---|---|
| `SUPPORTS_TOUCH` | ✅ **设了注入完全正常**（返回 `true`、目标屏 task 消失、触摸被 App 消费）—— 它是**产品决定**的必须位，永远会设 |
| `OWN_FOCUS` | ❌ **未观察到任何效果** —— 加它 / 去它，`FocusedDisplayId` 与主屏 `mCurrentFocus` **完全一致** |
| `STEAL_TOP_FOCUS_DISABLED` | ❌ 同上：**既不能替代 `OWN_FOCUS`，也不产生额外效果** |
| `ALWAYS_UNLOCKED` | ✅ **明确生效**（**2026-10-07 复验**，证据更硬）—— 锁屏态下：**含它** ⇒ 虚拟屏显示 App；**去掉它** ⇒ 虚拟屏**原始帧缓冲里就是锁屏界面**（大字时钟 + 日期，"内容被锁屏接管"）。复验补充两点：① `mCurrentFocus` **未必**翻转到 `com.android.systemui`（复验时它仍指向 App）⇒ 判据应看**画面内容**，不是焦点字段；② 设备**休眠**（`mWakefulness=Dozing`）时该屏是**全黑**，与"锁屏"是两种状态，排查时先用 `dumpsys power` 区分 |

⇒ **`OWN_FOCUS` 保留**（无害、属官方推荐组合），但它的"必须"理由**已降级**（§2 已改）。

### 6.5 已知行为（**不是 flag 能解决的**）

**虚拟屏上有前台 App 时**：

- `FocusedDisplayId` **变成虚拟屏** ⇒ **物理键盘 / 按键输入会进虚拟屏**；
- **本机屏的 `mCurrentFocus` 变成 `null`**（拆屏后恢复）。

⇒ 这是 Android 多显示的正常行为，**`OWN_FOCUS` 与 `STEAL_TOP_FOCUS_DISABLED` 都改不了** ✓
若将来成为问题，另想办法（不在当前范围）。

---

## 7. 常量真实数值（**仅供核对，代码里必须用常量名**）

本机 Android 16 / SDK 36 运行时反射所得，**不得复制进代码**：

| 虚拟屏位 | 值 | | 屏回读位 | 值 |
|---|---|---|---|---|
| `VIRTUAL_DISPLAY_FLAG_PUBLIC` | 1 | | `FLAG_SUPPORTS_PROTECTED_BUFFERS` | 1 |
| `VIRTUAL_DISPLAY_FLAG_PRESENTATION` | 2 | | `FLAG_SECURE` | 2 |
| `VIRTUAL_DISPLAY_FLAG_SECURE` | 4 | | `FLAG_PRIVATE` | 4 |
| `VIRTUAL_DISPLAY_FLAG_OWN_CONTENT_ONLY` | 8 | | `FLAG_PRESENTATION` | 8 |
| `VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR` | 16 | | `FLAG_ROUND` | 16 |
| `VIRTUAL_DISPLAY_FLAG_CAN_SHOW_WITH_INSECURE_KEYGUARD` | 32 | | `FLAG_CAN_SHOW_WITH_INSECURE_KEYGUARD` | 32 |
| `VIRTUAL_DISPLAY_FLAG_SUPPORTS_TOUCH` | 64 | | `FLAG_SHOULD_SHOW_SYSTEM_DECORATIONS` | 64 |
| `VIRTUAL_DISPLAY_FLAG_ROTATES_WITH_CONTENT` | 128 | | `FLAG_TRUSTED` | 128 |
| `VIRTUAL_DISPLAY_FLAG_DESTROY_CONTENT_ON_REMOVAL` | 256 | | `FLAG_OWN_DISPLAY_GROUP` | 256 |
| `VIRTUAL_DISPLAY_FLAG_SHOULD_SHOW_SYSTEM_DECORATIONS` | 512 | | `FLAG_ALWAYS_UNLOCKED` | 512 |
| `VIRTUAL_DISPLAY_FLAG_TRUSTED` | 1024 | | `FLAG_TOUCH_FEEDBACK_DISABLED` | 1024 |
| `VIRTUAL_DISPLAY_FLAG_OWN_DISPLAY_GROUP` | 2048 | | `FLAG_OWN_FOCUS` | 2048 |
| `VIRTUAL_DISPLAY_FLAG_ALWAYS_UNLOCKED` | 4096 | | `FLAG_STEAL_TOP_FOCUS_DISABLED` | 4096 |
| `VIRTUAL_DISPLAY_FLAG_TOUCH_FEEDBACK_DISABLED` | 8192 | | `FLAG_REAR` | 8192 |
| `VIRTUAL_DISPLAY_FLAG_OWN_FOCUS` | 16384 | | `FLAG_ROTATES_WITH_CONTENT` | 16384 |
| `VIRTUAL_DISPLAY_FLAG_DEVICE_DISPLAY_GROUP` | 32768 | | `FLAG_ALLOWS_CONTENT_MODE_SWITCH` | 32768 |
| `VIRTUAL_DISPLAY_FLAG_STEAL_TOP_FOCUS_DISABLED` | 65536 | | | |

> 两套命名空间**数值不同**（例如"可信"在虚拟屏位是 1024、在屏回读位是 128），
> 这就是"绝不硬编码"的另一条理由。

---

## 8. 变更记录

- **2026-10-06 初版**：从 [`agent/display.md`](display.md) 拆出（原文 §3）—— 该文件在回填实机验证
  结果后超出 [`docs.md`](../docs.md) §2 的单文件上限，按"内聚子主题整段提成同级文件"拆分。
- **2026-10-06 实测回填**：组合整体成立、回读校验、`DESTROY_CONTENT_ON_REMOVAL` 2×2 对照实验、
  常量真实数值表；原「待实机验证」中 4 项因需要 F5 而保留。
- **2026-10-06 决定**：`DESTROY_CONTENT_ON_REMOVAL` 由「PC 可配（缺省开）」**改为固定必须位**（§2）——
  依据是 §6.3 的对照实验：关掉后在**拆屏与崩溃两条路径**上都会把内容搬到本机屏并显示，
  不存在合理用途。§3「PC 可配」由 3 个减为 2 个。
- **2026-10-07 VDM 实施（双端实测）**：建屏改走**虚拟设备** ⇒ §4 整节重写 —— **不再派生
  `OWN_DISPLAY_GROUP`**（平台自动把设备屏放进设备组；解锁位因此被接受），并纠正"请求解锁位 ⇒
  平台强塞显示组"的旧误读（那位一直是我们自己派生的）。§3 的 `ALWAYS_UNLOCKED` 去掉"连带代价"；
  §6.2 回读期望由 5 位改为 **4 位**（无 `FLAG_OWN_DISPLAY_GROUP`，且该位在掩码内 ⇒ 一旦出现即建屏失败）。
  实测：真机 16 `frame=[0,0][1920,1800]` 满屏、锁屏与息屏两档照常出画、PC 端到端出画；
  AVD 15 同路径建屏成功、1080×1920 满屏（尺寸规则兜底）。
  ⚠️ 新增实测否证：`wm set-ignore-orientation-request -d <屏> true` **静默无效**（§4）。
- **2026-10-07 裁切机制纠正（源码级复核）**：§4 的因果链改写 —— 让屏无法满足固定方向请求的是
  **`mFixedToUserRotation=true`**（VDM 建的屏实测），**不是** `ignoreOrientationRequest`；
  裁切比例与"尺寸够竖（高÷宽 ≥ 1.22）"无关。细节整段提成
  [`agent/display-letterbox.md`](display-letterbox.md)（本文件只留摘要 + 链接，同时压回单文件上限）。
- **2026-10-08 产品决定：放弃"铺满"**：§3 的 `ALWAYS_UNLOCKED` 行与 §4 的因果链去掉"铺满由…保证"
  的表述，改为"**14/15 没有已启用的铺满手段**（产品已放弃该目标）"；14/15 的两条可选做法降级为
  "当前均未启用"的留档（见 [`agent/display-letterbox.md`](display-letterbox.md) §6）。
