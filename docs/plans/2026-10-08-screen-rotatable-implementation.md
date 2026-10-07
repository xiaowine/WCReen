# `:agent`：屏可旋转（A 方案）**实施记录**

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 的**实施记录**（方案文档保持原样，
> 偏差写在这里）。方案：[`2026-10-07-screen-rotatable.md`](2026-10-07-screen-rotatable.md)。
> 机制背景：[`agent/display-letterbox.md`](../spec/agent/display-letterbox.md) §6。

**状态**：**已实施 → 2026-10-08 当日回退**（产品决定放弃"铺满"这个目标，见 §0）。
留档目的：机制与实测结论仍然有效，将来若要重启这条路线可直接照此实施。

---

## 0. 回退记录（2026-10-08）

用户判断"允许屏旋转 ⇒ agent 还要检测 + 通知 PC"这条链路的**维护面不值**；B 方案（按 App 方向建屏）
也不实现 ⇒ **产品直接放弃"铺满"**：14/15 上 App 被裁按平台行为接受。

**回退范围**（代码已回到本方案之前的状态）：删除 `display/DisplayRotation.kt` 与其单测；
还原 `Screen.kt` / `Route.kt` / `RouteDeps.kt` / `Main.kt` / `ReaderLoop.kt` / `LinkLoop.kt` /
`proto/Control.kt` / `proto/Screen.kt`；PC 侧还原 `dto.rs`（`AppliedScreen.rotation`、`Event.rotation`、
`Event::SCREEN_ROTATION`）/ `session.rs` / `ui.rs` / `decode/src/render.rs`（单管线、无旋转）/ 协议测试。
**保留**：Android 16+ 的 `setIgnoreActivitySizeRestrictions(true)`（与本次机制无关，仍生效）。
验证：agent 单测 62 用例全绿 + `:agent:assembleRelease` 通过；PC `cargo test` 24 + 23 通过、无警告。

---

## 1. 落地形态

| 层 | 落点 | 做什么 |
|---|---|---|
| agent | `display/DisplayRotation.kt`（新） | `setRotatable()`：exec `cmd window fixed-to-user-rotation -d <id> disabled` + **回读确认**；`rotationOf()`：公开 API `Display.getRotation()`；`parseMode()` 纯函数（可单测） |
| agent | `display/Screen.kt` | 建屏回读通过后，**仅当尺寸开关没生效**时设可旋转；注册 `DisplayManager.DisplayListener`（回调在 `main`）⇒ 方向**变化**时回调 |
| agent | `runtime/Route.kt` | 把方向变化接到本路：`reader.reportEvent(EVENT_SCREEN_ROTATION, "", null, rotation)` |
| agent | `proto/Control.kt` / `proto/Screen.kt` | `EventMsg.rotation`（仅 `screenRotation` 带）；`AppliedMsg.rotation`（建屏那一刻的值） |
| agent | `runtime/ReaderLoop.kt` / `runtime/LinkLoop.kt` | `reportEvent(..., rotation)`；`applied.rotation = screen.rotation()` |
| agent | `runtime/RouteDeps.kt` / `Main.kt` | 把伪装 Context 造出的 `DisplayManager` 传给每路（监听要经它注册） |
| PC | `proto/src/dto.rs` | `AppliedScreen.rotation`、`Event.rotation`、`Event::SCREEN_ROTATION` |
| PC | `app/src/session.rs` | `Status.rotation`（来自 `applied` 与 `screenRotation` 事件）+ `Handles::rotation()` |
| PC | `decode/src/render.rs` | **4 份管线**（方向烘进着色器）+ `swaps_axes()`；`StreamTarget` 带方向、奇方向交换离屏目标长短边、`size()` 返回**转正后**尺寸 |
| PC | `app/src/ui.rs` | 方向变化**重建纹理**；状态栏显示「屏旋转 N°」 |

## 2. 与方案的差异（3 处，均为实施中发现）

| # | 方案原文 | 实际做法 | 为什么 |
|---|---|---|---|
| 1 | "最小版：建屏后读一次随 `applied` 回报" | **`applied` 仍报**（建屏那一刻的值，通常 `0`），但**真正的通道是 `screenRotation` 主动上报**（方案里的"完整版"） | 建屏时屏上**还没有 App** ⇒ 方向必然为 `0`；方向要等 App 拉起后由它请求驱动 ⇒ 只有"变化时上报"才能在正确时刻把值送到 PC |
| 2 | 未限定何时设可旋转 | **只在 `setIgnoreActivitySizeRestrictions` 没生效时**设（14/15） | 16+ 已经取消了活动级 letterbox，再旋转屏只会平白把内容转 90°（PC 得多转一次） |
| 3 | §6 判据 5「App 首次拉起即满屏（无需 force-stop 兜底）」 | **对可缩放 App 成立**；**对不可缩放 App 不成立**（首拉落 `SIZE_COMPAT_MODE`，重启一次即满屏） | 平台在"屏还没转"时就锁定了该 App 的 size-compat 尺寸。**未决项**见 §4 |

## 3. 实测证据（2026-10-08）

**AVD（Android 15 / SDK 35，屏 1920×1080@480，竖屏锁定 App）**：

| 观测 | 结果 |
|---|---|
| agent 日志 | `屏 43 已设为可旋转（disabled）：屏会转到 App 请求的方向` ✓ |
| 方向事件 | `屏 43 方向变为 3`（**注册时不再产生事件**：初值取注册那一刻的方向）✓ |
| 首次拉起（`FORCE_RESIZE_APP` 模拟可缩放） | ActivityRecord `[0,0][1080,1920]`、无 letterbox ✓ |
| 多尺寸 | `1024×768 → [0,0][768,1024]` ✓、`1600×900 → [0,0][900,1600]` ✓（均首拉即满屏） |
| PC 画面 | **正立**（状态栏 `屏旋转 270°`、`已上屏 23 解出 23 丢弃 0`）；截图见 §5 |
| 不可缩放（关掉模拟） | 首拉 `SIZE_COMPAT_MODE` 945×1080 ✗ → `am force-stop` 后重启 → `[0,0][1080,1920]` ✓ |

**真机（Android 16 / SDK 36，屏 1920×1800@560）**：

| 观测 | 结果 |
|---|---|
| agent 日志 | `建屏配置：sizeRestrictionsIgnored=true` ⇒ **不设可旋转**（无 `已设为可旋转` 行）✓ |
| 方向事件 | **不发**（方向没变）✓ |
| App | B 站 `[0,0][1920,1800]` 满屏 ✓；PC `c2.qti.avc.encoder 1920x1800@30`、`已上屏 1430 解出 1430 丢弃 0`、`显示 60.6 fps` ✓ ⇒ **无回归** |

## 4. 未决项（需用户决策）

**不可缩放 App 的首拉**：`resizeableActivity=false` 的 App 首拉会落到 `SIZE_COMPAT_MODE`
（平台在屏旋转前就锁定了兼容尺寸）。可选对策：

1. **接受并记录**（当前做法）：文档写明"这类 App 需要重启一次"；
2. **agent 侧自动重试**：方向变化后，若本屏顶层 Activity 仍被裁，则 `force-stop` + 重新拉起一次
   （agent 已有 `getAllRootTaskInfosOnDisplay` 能拿到顶层组件）—— 属**新行为**，需另行批准；
3. **PC 侧重发拉起**：PC 收到 `screenRotation` 后重发一次 `launch`（要求 PC 记住目标组件）。

## 5. 取证记录

- AVD 侧：`C:\Users\xiaow\AppData\Local\Temp\wcreen-a15\rot-*.log`、PC 窗口截图（画面正立、状态栏带
  `屏旋转 270°`）。
- 真机侧：`rot-real16b.log`（PC 日志）+ 上述 agent 日志。
- 设备状态已复原：模拟用的 `am compat disable FORCE_RESIZE_APP` 已关闭；两端 agent 与 PC 客户端均已停。

## 6. 变更记录

- **2026-10-08 初版**：实施完成（agent 10 处 + PC 5 处 + 单测 1 个），记录 3 处与方案的差异、
  双端实测证据与 1 项未决（不可缩放 App 的首拉）。
- **2026-10-08 当日回退**：见 §0 —— 产品决定放弃"铺满"，全部代码改动撤销（16+ 尺寸开关保留）。
  本文与方案文档**保留**，作为该路线的完整留档。
