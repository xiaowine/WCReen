# `:agent`：让虚拟屏**可旋转**（14/15 上"任意分辨率铺满"的唯一通路）

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 定义的**实现方案文档**。
> 范围：**只加一条"建屏后把屏设为可旋转"的动作 + 把屏的 rotation 如实上报**；
> 建屏路径（虚拟设备）、flags 组合、编码参数模型都不动。
> 写完即冻结；实施偏差另建文档。

**状态**：**草案（待批准）**（2026-10-07，用户要求"先记录，等下再实现"）。

---

## 1. 问题（14/15 上"任意分辨率铺满"目前无解）

裁切机制见 [`agent/display-letterbox.md`](../spec/agent/display-letterbox.md)：A15/14 上，**不可缩放 + 固定方向**的
App 在**方向不一致**的屏上会被裁进 **fixed-orientation letterbox**；三条同时成立才裁，其中第 3 条是
「**屏不会为它旋转**」。我们建的屏（虚拟设备路径与旧路径**都是**）实测 `mFixedToUserRotation=true`
⇒ 第 3 条恒成立 ⇒ 只能裁。

同批实测（模拟器 A15 / SDK 35，屏 1920×1080，竖屏锁定 App）**全部仍被裁**：

| 组合 | 结果 |
|---|---|
| VDM 屏 + `ignoreOrientationRequest=false` | 945×1080（`FIXED_ORIENTATION`）✗ |
| VDM 屏 + `ignoreOrientationRequest=true`（`wm set` 成功并回读确认） | 945×1080 ✗ |
| 旧路径屏（非 VDM）+ **默认显示组** + `ignore=false` | 945×1080 ✗ |
| 旧路径屏 + 默认显示组 + `ignore=true` | 945×1080 ✗ |

⇒ "关解锁位 / 留在默认显示组 / 置 `ignoreOrientationRequest`"（= 之前那套只在 Android 16 上成立的招式）
**在 14/15 上无效**。目前 14/15 只能靠"**按 App 方向建屏**"（竖屏 App 配 `高 ≥ 宽`）—— 但那样**尺寸不能任意**。

## 2. 机制：把第 3 条**打破**

`DisplayContent.handlesOrientationChangeFromDescendant(orientation)`（AOSP 15 `DisplayContent` L1715）
= `!shouldIgnoreOrientationRequest(orientation) && !getDisplayRotation().isFixedToUserRotation()`。

把屏的 **`fixed-to-user-rotation` 设为 `disabled`** ⇒ 该式为真 ⇒ 屏**会为 App 旋转** ⇒
`resolveFixedOrientationConfiguration()` 在 L9030 提前返回 ⇒ **不裁**，App 按旋转后的逻辑尺寸铺满。

## 3. 实测证据（2026-10-07，模拟器 A15 / SDK 35 + PC 客户端）

命令（`wm` 就是 `cmd window` 的壳：`/system/bin/wm` 内容为 `cmd window "$@"`）：

```
adb shell wm fixed-to-user-rotation -d <displayId> disabled   # 设置（实测成功）
adb shell wm fixed-to-user-rotation -d <displayId>            # 回读：disabled
```

| 观测 | 结果 |
|---|---|
| 屏状态 | `mRotation=3`、`cur=1080x1920`（逻辑方向翻转） |
| ActivityRecord | `bounds=[0,0][1080,1920]`、`areBoundsLetterboxed=false` ✓ **满屏** |
| App 自报 | `1080 x 1920 @ 480`（它确实看到竖屏） |
| 解锁位 | **不受影响**：设置前后 `dumpsys display` 的屏 flags 与显示组**逐字相同**（`FLAG_TRUSTED, FLAG_ALWAYS_UNLOCKED, FLAG_TOUCH_FEEDBACK_DISABLED`，`displayGroupId` 不变）⇒ 锁屏/息屏解耦照旧 ✓ |
| 编码 | 尺寸**不变**：仍 `1920x1080@12fps`（帧缓冲尺寸不变，只有内容转 90°） |
| PC 端 | `已上屏 66 解出 66 丢弃 0` —— 通路正常，但**画面侧躺 90°**（截图见 §7 取证记录） |
| 持久性 | **每次建屏都要重设**：新屏读回是 `default`（该设置随屏销毁，不跨屏保留） |

**时序坑（实测）**：必须在目标 App **首次**在该屏启动**之前**设好。若 App 继承到"上次被裁时锁定的
size-compat 尺寸"，会落到 `SIZE_COMPAT_MODE`（945×1080 ✗）；`am force-stop` 后重启同一屏即满屏 ✓。

## 4. 方案

### 4.1 agent 侧（小）

1. **建屏成功后立刻**：exec `cmd window fixed-to-user-rotation -d <displayId> disabled`，
   并**回读确认**（`wm fixed-to-user-rotation -d <id>` 或 `dumpsys window displays` 的
   `mFixedToUserRotation=`）—— 遵循全局规则「设置参数必须回读确认」。
   - 放哪：`display/Screen.kt`（建屏收尾处）或新 `display/DisplayRotation.kt`；
     命令执行参照**既有先例** `display/CompanionAssociation.kt`（`ProcessBuilder` 调 `cmd companiondevice`）。
   - 失败形态：**可降级**（拿不到就退回"按 App 方向建屏"，只告警一次）—— 与
     [`wrappers/ActivitySizeRestrictions`](../../agent/src/main/kotlin/com/xiaowine/wcreen/agent/wrappers/ActivitySizeRestrictions.kt) 同级。
2. **读取 rotation**：`Display.getRotation()`（**公开 API**，`VirtualDisplay.display.rotation`）——
   不需要解析 `dumpsys`。
3. **上报**：最小版 = 建屏+拉起 App 后读一次，随 `applied` 回报（新增字段 `rotation`，0/1/2/3）；
   完整版 = 注册 `DisplayManager.registerDisplayListener`，rotation 变化时**主动上报**
   （线程归口按 [`agent/runtime-threads.md`](../spec/agent/runtime-threads.md)）。

### 4.2 PC 侧（中）

帧尺寸不变（仍是 W×H），只需**显示时按 rotation 旋转**：

- `pc/app/src/ui.rs` 的 `StreamTarget` / wgpu 管线：按 rotation 做 UV 旋转（0/90/180/270）；
- 画面比例随之变化（内容变竖屏）⇒ 窗口/布局的宽高比要跟着走（现有布局已对画面做留边，改动有界）。

### 4.3 协议与文档（小）

- `applied` 增加 `rotation`（或独立主动上报消息）→ [`transport-shapes.md`](../spec/agent/transport-shapes.md) /
  [`transport-messages.md`](../spec/agent/transport-messages.md) /
  [`transport-changelog.md`](../spec/agent/transport-changelog.md)；PC 侧 `proto/src/dto.rs` 同步；
- [`agent/display-letterbox.md`](../spec/agent/display-letterbox.md) §6、
  [`display.md`](../spec/agent/display.md)、[`display-changelog.md`](../spec/agent/display-changelog.md)
  补"可旋转"这条路；
- [`code-map.md`](../code-map.md) 若新增文件则登记。

## 5. 改动清单（预估）

| 层 | 文件 | 改动 |
|---|---|---|
| agent | `display/DisplayRotation.kt`（新）或 `display/Screen.kt` | 建屏后设 `fixed-to-user-rotation=disabled` + 回读 |
| agent | `display/Screen.kt` / `runtime/LinkLoop` | 读 `Display.getRotation()` 并回报 |
| agent | `proto/Screen.kt`（`AppliedMsg`） | 新增 `rotation` 字段 |
| agent | 单测 | 命令拼装 / 回读解析 / rotation 映射 |
| PC | `app/src/ui.rs`（+ 可能的 `decode`/`proto`） | UV 旋转 + 比例跟随 |
| PC | `proto/src/dto.rs` | `rotation` 字段 |
| 文档 | 上列 5 份 + `code-map.md` | 同步 |

## 6. 验收判据（可观测，不接受"没报错"）

1. **铺满**：竖屏 App 在**横屏**屏上（如 1920×1080）`dumpsys activity activities` 里
   `areBoundsLetterboxed=false`，ActivityRecord `bounds` = 旋转后的整屏（`[0,0][1080,1920]`）；
2. **尺寸任意**：至少验证 1920×1080 / 1600×900 / 1024×768 三个横屏尺寸都满屏（对照 §1 的 945×1080 ✗）；
3. **画面正确**：PC 窗口里 App 内容**正立**（不是侧躺），且 `已上屏 N 解出 N 丢弃 0`；
4. **不回归**：竖屏尺寸（1080×1920 / 1080×1080）仍满屏；真机 16（有 `setIgnoreActivitySizeRestrictions`）
   行为不变；锁屏/息屏解耦仍成立（像素级两档取证）；
5. **时序**：App 首次拉起即满屏（无需 force-stop 兜底）；重连/换尺寸后同样成立。

## 7. 风险与待定项

- **内容转 90° 是这条路的固有代价**：换来"尺寸任意"，代价是"PC 必须跟转 + 窗口比例变竖屏"。
  若只需"铺满"，**"按 App 方向建屏"零代码且内容正立** ⇒ 建议先评估是否真的需要任意尺寸。
- **`cmd window` 依赖 shell 权限**：agent 以 shell（uid 2000）跑，实测同一 uid 下 `wm`/`cmd window` 可用；
  但**未在 agent 进程内实跑**（[INFERENCE]），实施时先做一次冒烟。
- **rotation 动态变化**（App 自己改方向、系统弹窗）：最小版只在建屏后读一次，可能过时 ⇒ 需要完整版监听。
- **PC 窗口比例策略**：竖屏内容要不要把窗口自动转成竖屏，属 UX 决策，未定。
- **跨平台**：PC 侧旋转要在 Win / macOS / Linux 三端都成立（wgpu 层做则天然一致）。
- 取证记录：`C:\Users\xiaow\AppData\Local\Temp\wcreen-a15\`（`A-rotated-frame.png` = PC 窗口侧躺满屏画面截图、
  `wr-A.log` / `wr-A2.log` = PC 客户端日志）。

## 8. 变更记录

- **2026-10-07 草案**：记录 A 方案（让屏可旋转）的机制、实测证据、改动清单、验收判据与风险。
  **未实施**（用户要求先记录）；同期实测的"死路"四组组合与 B 方案取舍见
  [`agent/display-letterbox.md`](../spec/agent/display-letterbox.md) §6。
