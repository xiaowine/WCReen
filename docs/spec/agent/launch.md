# `:agent` · 拉起应用（F2）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/module.md`](module.md) §2 的子系统 #4。
> 覆盖功能 **F2（在虚拟屏上跑 App）**；**输入回注（F5）见 [`agent/input.md`](input.md)**。
> 硬约束见 [`agent/module.md`](module.md) §3；屏参数与 flags 见 [`agent/display.md`](display.md)。

---

## 1. 范围

F2 = **两条互相独立的指令**，均可在**任意时刻**下发：

| 指令 | 语义 |
|---|---|
| **枚举可启动应用** | 把"能打开的软件名单"交给 PC |
| **打开目标应用** | 在目标屏上拉起指定组件 |

**边界**：

- 消息的字节格式与传输由 **F4** 定（**不做分片** —— 见 [`agent/transport-protocol.md`](transport-protocol.md) §5）；本文件只定义**语义与字段**。
- 输入回注属 **F5**；`force-stop` 等生命周期动作属 **F6**。
- **agent 不维护业务状态** —— "当前打开的是哪个 App"由 **PC 记忆**，agent 只按事实回报。

---

## 2. 指令模型（两条指令共同遵守）

| 约束 | 说明 |
|---|---|
| **任意时刻** | 不依赖建屏、不依赖连接时长、可重复下发。**枚举无屏也能用** |
| **无状态** | agent **不记忆**上次打开的是谁；断开重连后由 PC 按正常流程重发，agent 因此更简单 |
| **请求-回报** | 每条指令带 `requestId`，结果**异步回报**并携带同一 id（PC 可连发 / 重发） |
| **判据独立** | 子进程的 stdout 与退出码**一律不作成功判据**（见 §5）；成功与否由可观测事实判定 |
| **不阻塞** | 命令处理与编码 / 传输 / 心跳并发，不得阻塞帧通路（见 [`agent/module.md`](module.md) §3 不变量 6、F7） |
| **目标屏标识** | **一条连接 = 一块屏**；指令**不带 displayId**，多路 = 多条连接 |
| **就绪时机** | 命令 handler 必须在**建立连接之前**就绪（不变量 5：读线程在 `accept()` 返回那刻就开跑） |

> 「是否允许**无屏连接**」是连接拓扑问题，归 **F4**；本文件只声明**枚举指令不依赖屏**。

---

## 3. 枚举可启动应用

**细节见 [`agent/launch-enumerate.md`](launch-enumerate.md)**：请求参数（`forceRefresh`）、
实现路径（进程内 `ApplicationPackageManager`）、返回字段、范围与缓存、**已实测的名单口径**。

要点速览：只列 `MAIN` + `LAUNCHER` 入口；agent 侧缓存 + `forceRefresh` 强制重查；
**不依赖屏**，可在任意时刻下发。

---

## 4. 打开目标应用

### 4.1 请求

| 参数 | 可否缺省 | 说明 |
|---|---|---|
| 组件名 | **否** | `包名/组件名`，来自枚举名单 |
| 目标屏 | 不需要 | 由连接决定（§2） |

**可重复下发 = 切换目标应用**。不指定任何组件时，agent **什么都不做**（见 §6）。

### 4.2 实现路径（已定）

子进程调 `cmd activity start-activity --display <id> -a android.intent.action.MAIN -c android.intent.category.LAUNCHER -n <组件>`。

本机实测：`--display <DISPLAY_ID>` **存在**；`/system/bin/am` 只是 320 字节的 sh 包装，内部就是 `cmd activity "$@"`
⇒ **直接调 `cmd activity`**，少一层 shell。agent 本身是 uid 2000，子进程权限天然一致；拉起是低频操作，进程开销可忽略。

不采用 `-W`（help 标注 `(initial display)`，语义存疑）⇒ **用轮询**取判据。

**不用**进程内 `IActivityTaskManager.startActivity`：其签名逐版本漂移，维护成本高（原型亦为此否决）。

#### 4.2.1 「同一 App 能否同时出现在两块屏」——已实测，并已决定

| 拉起方式 | 结果 |
|---|---|
| **默认**（`--display N`） | 目标组件**已有 task** 时，往另一块屏拉起 = **把该 task 整体搬过去**（**同一个 `ActivityRecord`**），原屏变空；task 内状态（当前页面、滚动位置等）**随 task 一起保留** |
| 加 **`--activity-multiple-task`** | 在另一块屏创建**第二个实例**：两个不同的 task 与 `ActivityRecord`，两块屏**同时**显示 |
| 两个**不同** App | 各自独立，**可同时**在各自屏上显示 ✓ |

**实测规模**：多实例**至少 8 份**同时成立（未触及上限）；且 8 份**共用同一个进程**
（`app=ProcessRecord` 完全相同）⇒ 界面各自独立，但**进程内全局状态是共享的**。

**决定：PC 侧限单实例 ⇒ agent 一律不加 `--activity-multiple-task`。**

理由与后果：

- 「同一 App 在**任一时刻只存在一份**」由 framework 的默认搬移语义**自动保证**，agent 不需要额外逻辑；
- 目标屏**已有**该 App ⇒ 拉起是无害复用（`delivered to currently running top-most instance`），
  **屏内换页由 App 自己的导航负责**（属 F5 输入，不由 agent 代劳）；
- 目标屏**没有、但别的屏有** ⇒ **搬过来**（连同 task 内状态），**原屏变空** ——
  **PC 必须知道这一点**：App 的位置会随请求漂移，PC 侧维护的状态要跟着更新。

> 推论：**协议里不需要"复用还是新建"这种字段** —— 单实例决策已消除该分叉（§4.1 的请求参数保持最小集）。

**附带观测**：默认搬移时 `cmd activity` 会输出
`Warning: Activity not started, intent has been delivered to currently running top-most instance.` 且**退出码 0**
—— 是"被受理但没新建实例"，**不能当成功判据**（见 §5）。

### 4.3 成功判据（可观测）

**目标屏确有该 task，且其顶层包名 == 目标包名。**

**判据来源（已实测，进程内即可拿到，不需要解析 dumpsys）**：

反射 `android.app.ActivityTaskManager.getService()` → `IActivityTaskManager`，调用
**`getAllRootTaskInfosOnDisplay(int displayId)`** 返回结构化 `RootTaskInfo`，可直接读到
`taskId` / `displayId` / `topActivity` / `baseActivity` / `isVisible` / `numActivities`。

同接口还有 `getRootTaskInfoOnDisplay(int,int,int)`、`getTopVisibleTaskOnDisplay(...)`、
`isActivityStartAllowedOnDisplay(int, Intent, String, int)` 可选用。

> ⚠️ 若退路要解析 `dumpsys activity activities`：**段顺序会随聚焦屏变化**（聚焦屏排最前），
> 必须**按 `Display #N` 段头切分**，不能假设出现次序。

**组件名只能判到包名级别 —— 必须允许入口转发。** 本机两次独立实证：

| 请求的组件 | 屏上真实的顶层 | 说明 |
|---|---|---|
| `com.android.settings/.Settings` | `com.android.settings/.MiuiSettings` | 转发入口，自己又发了隐式 `android.settings.SETTINGS` |
| `com.github.android/.main.MainActivity` | `com.github.android/.auth.SimplifiedLoginActivity` | 同上 |

两者 `am` 都报"成功"，且退出码 0。⇒ 以组件名做严格相等会**误判失败**；
以 `am` 的输出做判据会**掩盖真实落地组件**。

---

## 5. 失败分类

| 类别 | 可观测判据 |
|---|---|
| 未安装 / 无启动入口 | 不在枚举名单；发起时查不到组件 ⇒ 直接拒绝并说明 |
| 被系统拒绝 | 目标屏无新 task 且超时 |
| 起来了又立刻退出 | task 出现过又消失 / 进程不在 |
| **落到别的屏** | 目标屏无该 task，而**默认屏**有 ⇒ **违反"不影响本机屏"的承诺，必须显式上报** |
| 成功 | 目标屏顶层包名 == 目标包名（§4.3） |

> 全部判据必须来自**可观测信号**，不得是"没报错"。

### 5.1 子进程信号的真实能力边界（已实测）

| 观测 | 结果 |
|---|---|
| 退出码 | 成功 **0** / 失败 **1** |
| 失败输出 | **三种失败完全相同**：已禁用包、包不存在、包在但组件名错，一律 `Error type 3` + `Activity class {...} does not exist.` ⇒ **退出码只能判"是否被受理"，判不出原因** |
| `EXIT=0` 的含义 | 仅表示**被受理**：可能是"投递给已存在的顶层实例"的 no-op（§4.2.1），**不表示换了 App** |
| 失败后屏的状态 | 三种失败**都不产生新 task**，目标屏顶层不变（无副作用） |
| `isActivityStartAllowedOnDisplay` | 实测对**已禁用包也返回 `true`** ⇒ **不能替代名单校验**（它答的是"这块屏允不允许拉起"，不是"这个组件能不能起来"） |

⇒ 因此：**"未安装 / 被禁用 / 组件名错"必须在发起前用枚举名单挡掉**，不能靠发起后的信号区分。

---

## 6. 明确不做

- ❌ **隐式默认启动** —— 不指定组件就不拉起任何东西，PC 显式表达意图；
- ❌ agent 记忆"上次打开的 App"（归 PC）；
- ❌ "停止 App"（`force-stop` 归 **F6** 生命周期）；
- ❌ **创建同一 App 的第二个实例**（不加 `--activity-multiple-task`）—— 单实例是 PC 侧的既定约束，
  理由与后果见 §4.2.1；
- ❌ 跨用户；
- ❌ v1 下发图标。

---

## 7. 实机验证结果

**已全部验证完毕（V1–V6 全有结论，且已回填进本文件各节）**
—— 数据见 [`agent/launch-verified.md`](launch-verified.md)（因超出单文件上限已提成独立文件）。

---

## 8. 本功能产出的参数

枚举 TTL、是否含非 LAUNCHER 入口、拉起超时、轮询间隔。

> ⚠️ **这几项都是「实现常量」，不暴露给 PC** —— F8 撤销后，**agent 侧不再有集中收纳参数的文档**；
> PC 可配的业务参数**一律由 `applyScreen` 承载**（[`agent/transport-messages.md`](transport-messages.md) §2.3）；「是否含非 LAUNCHER 入口」的口径见 [`agent/launch-enumerate.md`](launch-enumerate.md)。

---

## 9. 变更记录

- **2026-10-06 初版**：F2 定稿。确立**两条独立指令**（枚举 / 打开），均可在**任意时刻**下发、互不依赖；
  枚举走**进程内 PackageManager + agent 侧缓存 + `forceRefresh` 获取参数**（非独立指令）；
  拉起走 `cmd activity --display` + **独立可观测判据**（顶层**包名**，允许入口转发）；
  目标屏标识取"**一条连接 = 一块屏**"；**枚举无屏也能用**；
  agent **不记忆**目标应用（交 PC）；记录 6 项待实机验证。
- **2026-10-06 实机验证完毕**：V1–V6 全部有结论（§7），并据此**回填**——
  §3.4 名单口径（禁用包不入名单、label 仍可取、`FLAG_SYSTEM` 判系统应用）—— 该节已拆至
  [`agent/launch-enumerate.md`](launch-enumerate.md)；
  §4.2.1 **`--display` 的搬移语义**与 `--activity-multiple-task` 的复制语义；
  §4.3 判据来源改为进程内 `getAllRootTaskInfosOnDisplay`，并补第二次入口转发实证；
  §5.1 子进程信号的**真实能力边界**（退出码判不出原因，`isActivityStartAllowedOnDisplay`
  不挡禁用包）。同时确认 F1 验收判据 1–5 全部成立（见 [`agent/display.md`](display.md) §5）。
- **2026-10-06 决定**：**PC 侧限单实例** —— agent 一律**不加** `--activity-multiple-task`。
  补充实测：多实例**至少 8 份**（未触上限）且**共用同一进程**（界面独立、进程内状态共享）；
  默认搬移会**连同 task 内状态**一起迁移，**原屏变空**。理由与后果见 §4.2.1，§6 增加对应禁忌。
- **2026-10-06 拆分**：实机验证结果整段提成 [`agent/launch-verified.md`](launch-verified.md)（原文件超单文件上限），结论已回填进 §3.4 / §4.2.1 / §4.3 / §5.1。
