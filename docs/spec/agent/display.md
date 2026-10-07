# `:agent` · 显示源（F1）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/module.md`](module.md) §2 的子系统 #2。
> 覆盖功能 **F1（建独立虚拟屏，扩展模式）**；**编码（F3）见 [`agent/encode.md`](encode.md)**。
> 硬约束见 [`agent/module.md`](module.md) §3；PC 侧对应约束见 [`tech-stack/pc.md`](../tech-stack/pc.md)。

---

## 1. 范围与模式

- 当前**只做扩展模式**：新建一块独立虚拟屏，**不影响本机屏**。
- **镜像模式（捕获现有屏）不在范围内** —— 属于后续独立功能，单独讨论。
- 不使用 `MediaProjection`（硬约束）。

---

## 2. 参数模型：上游决定，缺省由 agent 按设备推导

### 2.0 参数的载体：一条「**申请屏幕**」指令

**建屏所需的全部参数都集中在这一条指令里**，由 **PC 选择**；**PC 拿这条指令来申请屏幕**。

| 组 | 参数 | 定义在哪 |
|---|---|---|
| **画质 / 编码** | 分辨率（尺寸）、**刷新率**、码率（bpp 系数或绝对值）、**格式**（codec / 位深 / profile / 色彩） | 本文件 §2；[`agent/encode.md`](encode.md)（码率 §4、低延迟 §3、**格式组** §5） |
| **屏行为** | flags（7 个必须位 + 2 个 PC 可配；不请求显示组） | [`agent/display-flags.md`](display-flags.md) |
| **其他** | DPI、屏名 | 本文件 §2 |

**推论（重要）**：

- **建屏是一条显式指令，不是"连上就建屏"** —— 连接的建立与建屏**解耦**，连接可以先存在而没有屏
  （与 F2 的「**枚举无屏也能用**」一致，见 [`agent/launch.md`](launch.md) §2）；
- 所以 `display-lifecycle.md` §1 的"由上游请求触发"**指的就是这条指令**；
- **字段的字节格式、消息序号与错误码归 F4** —— 本文件与 [`agent/encode.md`](encode.md) 只锁**语义**；
- 重连时 PC 会**重发这条指令** ⇒ 指令必须能作用于**已存在的屏**：
  可变参数（码率）直接应用，不可变的（尺寸 / 刷新率 / flags）**回报"需重开会话"**（见 §2.1）。

⇒ **屏行为 flags 一并并入这条指令**（已定）—— 它同样是建屏必需参数，不另开一条。

参数由**上游（PC 用户的请求）**决定；**协议允许缺省**；
缺省时由 agent **按运行设备推导**（**绝不写死常量**），并在回报里给出**实际生效值**。

链路固定为：

```
上游请求（模式 + 参数，可部分缺省）
   → agent 校验可行性（编码器能力 / 平台限制）
   → 应用
   → 回报【实际生效值】；不可行 → 【带原因的拒绝】
```

| 参数 | 可否缺省 | 缺省来源（按运行设备推导） | 约束 |
|---|---|---|---|
| 模式 | 否（本阶段固定为扩展） | — | 镜像不在范围 |
| 尺寸 `w×h` | 可 | **本机主屏分辨率** | 必须落在编码器支持的尺寸范围与对齐内；**编码尺寸必须 = 屏尺寸**（硬约束：不允许 GPU 缩放，省性能）。推论：**屏尺寸的可行性 = 编码器的能力**，尺寸请求按编码器能力校验 |
| DPI | 可 | **本机主屏 densityDpi** | 只给尺寸不给 DPI 时，密度按尺寸比例缩放（否则屏内 App 的 UI 元素占比会变） |
| **刷新率** | 可 | **60 Hz** | **由 PC 选择**。**建屏期参数、会话内固定** —— 运行期**没有**接口可改（见 §2.1）。实测**精确采纳**（请求 30 / 24 / 90 / 120 全部生效，且各生成独立 `Display.Mode`）。它决定屏的合成与出帧节奏 ⇒ **它也是"帧率"的唯一控制手段** |
| flags：**必须位** | 否（固定） | — | [`agent/display-flags.md`](display-flags.md) §2 的**七位**固定设置，不可配 |
| flags：`SHOULD_SHOW_SYSTEM_DECORATIONS` | 可 | **关**（不显示系统装饰） | PC 可配；需 `TRUSTED` |
| flags：`DESTROY_CONTENT_ON_REMOVAL` | **否**（固定必须位） | — | **不可配**：关掉后拆屏或崩溃都会把内容搬到本机屏并显示（见 [`agent/display-flags.md`](display-flags.md) §6.3） |
| flags：`ALWAYS_UNLOCKED` | 可 | **开**（主屏锁屏 / 息屏都不接管本屏） | PC 可配；**无连带代价**（屏挂在虚拟设备上，不再派生显示组）。**铺满不是目标**（2026-10-08 产品决定放弃，见 §3 与 [`agent/display-letterbox.md`](display-letterbox.md) §6）；Android 16+ 的尺寸开关仍启用 |
| flags：`OWN_DISPLAY_GROUP` | **不请求** | — | **不暴露给 PC**；屏由虚拟设备创建 ⇒ 平台自动把它放进设备自己的显示组（见 [`agent/display-flags.md`](display-flags.md) §4）。回报里的 `ownDisplayGroup` **恒 false** |
| 屏名 | 可 | 固定前缀 + 序号（`WCReen-<n>`） | 便于 `dumpsys display` 排查与多路区分 |

> 「缺省」只影响**取值**，不影响**责任**：实际生效的每一个值都必须回报给上游。

### 2.1 参数变更怎么处理

参数**只可能在连接建立时**下发（重连走正常连接流程，见 [`agent/display-lifecycle.md`](display-lifecycle.md) §2）。按"能否原地生效"分三类：

| 类别 | 参数 | 变更处理 |
|---|---|---|
| **重建编码器**（**不存在热改**） | 码率、码率模式、帧率、关键帧间隔、格式组 | 流程见 [`agent/encode-lifecycle.md`](encode-lifecycle.md) §2.1：`setSurface(null)` → release 旧 → 新建 → `setSurface(新面)` → **请求同步帧** |
| **重建编码器 + 改屏（必须成对改）** | 尺寸 | 同上，且要 `resize()` 改屏；**两者必须同步改、始终相等**（不允许出现尺寸不匹配的窗口） |
| **只改屏** | DPI（像素数不变） | `resize(当前 w, h, 新 dpi)`；**编码器不动** |
| **不可原地改** | flags、**刷新率** | 只在建屏时给 ⇒ **会话内固定**；PC 请求变更时**回报"需重开会话"**（本轮不做显式重建屏） |

> 原则：**绝不拆屏** —— 屏与屏上 App 的状态必须保住；参数变更**只重建被牵连的那一环**
> （编码器，必要时连屏尺寸一起改）。

> ⚠️ **对容量 / 降级策略的推论（已对齐）**：既然**不存在热改**，agent 侧**运行期能改的参数一个都没有** ——
> 码率变更也要**重建编码器**（不拆屏，但会短暂停流 + 补 IDR）。
> ⇒ **不存在"降级"这条路**。
> PC 侧**不做任何容量处理**（[`tech-stack/pc.md`](../tech-stack/pc.md) §3）⇒ 两侧口径一致：
> **没有「拒绝新增路」这回事** —— 容量不足表现为**每路 fps 下降**，不拒绝、不降级、不踢路。

### 2.2 建屏的前置：进程内 Context 引导（已实测的**五条**硬性要求）

`app_process` 里没有 Application，必须先造出可用 Context。**以下五条都是实测踩出来的**，
任一条不满足都表现为"看起来莫名其妙的失败"：

| # | 要求 | 不满足时的现象 |
|---|---|---|
| 1 | **主线程必须先建好 Looper，再构造 `ActivityThread`** —— 用 **`Looper.prepare()` + 反射把 `Looper.sMainLooper` 设为它**（**不要用 `Looper.prepareMainLooper()`**，理由见下） | `RuntimeException: Can't create handler inside thread Thread[main,5,5,main] that has not called Looper.prepare()`（`ActivityThread` 构造里会 `new Handler`） |
| 2 | **取 `DisplayManager` 必须用隐藏构造器 `DisplayManager(Context)`** 把伪装 Context 塞进去，**不能**用 `getSystemService()`（现在只用于"读主屏缺省尺寸/密度"；**建屏走虚拟设备**，见 §3） | `SecurityException: packageName must match the calling uid` —— `getSystemService()` 返回的实例绑的是**真系统 Context**（包名 `android`），不是我们的伪装 |
| 3 | 包名必须伪装成 **`com.android.shell`**（与进程真实 uid 2000 一致） | system_server 侧权限 / 归属判定失败 |
| 4 | **attribution 也要伪装成 `com.android.shell`**（反射改写 `ContextImpl.mAttributionSource`，并回读确认） | 建**虚拟设备**时服务端按 attribution 包名查 CDM 关联 ⇒ 找不到关联而失败（真系统 Context 报 `android`） |
| 5 | **伪装 Context 必须覆写 `getApplicationContext()` 返回自身** | 建虚拟设备时 NPE：`Attempt to invoke virtual method Context.getAttributionSource() on a null object reference`（`VirtualDeviceInternal` 构造经它取 attribution，而 `app_process` 里没有 Application） |

**引导链（顺序固定，已跑通）**：

```
Looper.prepare() → 反射把 Looper.sMainLooper 设为它
  → 反射构造 ActivityThread，写入 sCurrentActivityThread、mSystemThread=true
  → 填 mConfigurationController（SDK ≥ 31）
  → 填 mBoundApplication（AppBindData.appInfo.packageName = com.android.shell）
  → 反射调 ActivityThread.getSystemContext() 拿到系统 Context
  → 反射改写 attribution（mAttributionSource，包名 = com.android.shell）
  → 包一层伪装包名的 Context 包装（并覆写 getApplicationContext() 返回自身）
  → 填 mInitialApplication
```

> ⚠️ **为什么不用 `Looper.prepareMainLooper()`（本机实测，实体机与 AVD 结论一致）**：
> 两者**都能完成引导、都能建屏**（`ActivityThread` 构造只要求当前线程有 Looper），差别只在**能否退出** ——
> `prepareMainLooper()` 建的 Looper **`quitAllowed = false`** ⇒ 主线程**无法退出**：
> `quitSafely()` 抛 `IllegalStateException: Main thread not allowed to quit.`，
> `Looper.loop()` **永不返回**，进程只能被强杀。
>
> 改用 `Looper.prepare()` 建一个**可退出**的 Looper，再反射补上 `sMainLooper`
> （**必须补**：不补则 `Looper.getMainLooper()` 为 null，部分框架路径会炸），实测：
> `myLooper == getMainLooper` ✓、建屏正常 ✓、**`quitSafely()` 成功且 `loop()` ~300ms 内返回** ✓。
>
> 这是 **F6「编排与收尾」的前提** —— 没有它就没有"优雅退出"这条路，只能靠强杀。

> 隐藏 API 的版本差异**必须集中封装**（见 [`agent/module.md`](module.md) §8 禁忌），不得散落各处。

---

## 3. flags

**细节见 [`agent/display-flags.md`](display-flags.md)** —— 按语义分档的完整清单
（必须设置 / PC 可配 / 派生 / 明确不设）、`DESTROY_CONTENT_ON_REMOVAL` 的 2×2 对照实验、
常量真实数值表。

要点速览：**必须位 7 个**（`TRUSTED`、`PUBLIC`、`OWN_CONTENT_ONLY`、`SUPPORTS_TOUCH`、
`OWN_FOCUS`、`TOUCH_FEEDBACK_DISABLED`、`DESTROY_CONTENT_ON_REMOVAL`）；**PC 可配 2 个**
（`SHOULD_SHOW_SYSTEM_DECORATIONS` 缺省关、`ALWAYS_UNLOCKED` **缺省开** —— 保住"主屏锁屏/息屏时虚拟屏照常可用"；
**屏挂虚拟设备 ⇒ 该位不再有任何连带代价**，见 [`agent/display-flags.md`](display-flags.md) §3、§4）；
**不请求显示组**（平台自动给设备屏分设备组）；**明确不设 7 个**。**一律用常量名拼，绝不硬编码数值**。

**建屏路径**：屏挂在**虚拟设备**上（`wrappers/VirtualDevice.kt`：CDM 关联 → 建虚拟设备 → 挂屏），
这是"锁屏/息屏不接管"能成立的路径；建屏机制的因果链与双端实测见
[`plans/2026-10-07-vdm-mechanism-corrected.md`](../../plans/2026-10-07-vdm-mechanism-corrected.md)。

**关于"铺满"**：**产品决定（2026-10-08）放弃这个目标** —— 14/15 上 App 若被裁（fixed-orientation
letterbox / size-compat），按平台行为接受，不做额外动作；机制、实测过的可选做法（让屏可旋转 / 按 App
方向建屏）与它们的代价见 [`agent/display-letterbox.md`](display-letterbox.md) §6。
**Android 16+** 仍启用尺寸开关 `setIgnoreActivitySizeRestrictions(true)`（[`agent/code-hiddenapi.md`](code-hiddenapi.md) §3 第 6 处），
实测 1920×1800 满屏。

---

## 4. 生命周期

见 [`agent/display-lifecycle.md`](display-lifecycle.md) —— 建立 / 断开与重连 / 冻结候选 / 待实机 / 真正结束与异常。

---

## 5. 验收（F1 单独闭环）

用**假消费者**独立验收，不依赖 F3：

| # | 判据 | 实测结果 |
|---|---|---|
| 1 | 系统侧能查到这块虚拟屏（不是"进程没报错"） | ✅ 建屏返回 `displayId`，`dumpsys display` 里查到该屏（含 `uniqueId` / 尺寸 / 密度 / `owner com.android.shell`） |
| 2 | 它带可信标记：第三方 App 能在其上启动 | ✅ 普通第三方 uid 的 App `RESUMED`；回读 flags 精确等于请求的高位（见 [`agent/display-flags.md`](display-flags.md) §6.2） |
| 3 | 输出面**有帧到达** —— 挂一个只计数、不编码的消费者，计数在涨 | ✅ `ImageReader` 消费者计数递增（一次测试 162 → 256） |
| 4 | **本机屏不受影响** —— 建屏前后本机 display 状态不变 | ✅ 本机屏段零污染：home / 壁纸 / SystemUI 仍在，无外来 task |
| 5 | **无缩放**：输出面尺寸 == 屏尺寸（尺寸不等时必须判失败） | ✅ 消费者报告的 buffer 尺寸 **== 屏尺寸**（1280×720 建屏 → 帧 1280×720） |

**参数侧同时验过**：DPI 参数真实生效（请求 320 ⇒ `dumpsys display` 里 `density 320`）；
尺寸由编码器能力校验**已定**（见 [`agent/encode-probe.md`](encode-probe.md) §5）。

> 判据必须是**可观测信号**，不能是"没报错"（见 [`tech-stack/pc.md`](../tech-stack/pc.md) §2 的同类教训）。

---

## 6. 编码（F3）

**已定** —— 见 [`agent/encode.md`](encode.md)（入口）及其 6 个子文档。

---

## 7. 变更记录

**见 [`agent/display-changelog.md`](display-changelog.md)**（本记录因超出单文件上限已提成独立文件）。
