# `:agent` · 输入注入（F5）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/module.md`](module.md) §2 子系统 #4 的**输入部分**。
> 覆盖功能 **F5（接收并注入输入）**。拉起 App 见 [`agent/launch.md`](launch.md)；协议格式见
> [`agent/transport-protocol.md`](transport-protocol.md)；消息字段见 [`agent/transport-messages.md`](transport-messages.md)。
> **变更记录记在本文件 §9。**

**状态**：**已定**（2026-10-07）。

---

## 1. 实测前提（**全部通过**）

| # | 前提 | 实测结果 |
|---|---|---|
| 1 | **注入权限** | ✅ uid 2000 的 `app_process` **可以** `injectInputEvent`（返回 `true`） |
| 2 | **定向到虚拟屏** | ✅ 注入 BACK 到虚拟屏 ⇒ **该屏的 App 退出**（task 消失） |
| 3 | **本机屏不受影响** ⭐ | ✅ 同一时刻**主屏 top 完全不变** —— [`agent/display.md`](display.md) §5 验收判据 4 成立 |
| 4 | **触摸真的被消费** | ✅ 注入 down+5×move+up 后 1.5s 内屏**新增 34 帧** |

**取法（S4 实测定稿）** —— 登记在 [`agent/code-hiddenapi.md`](code-hiddenapi.md) §3 第 5 处：

| 环节 | 手段 |
|---|---|
| 取 `InputManager` | **公开**的 `getSystemService(INPUT_SERVICE)` —— **0 反射**（与建屏不同：注入只看调用进程 uid，不看 Context 包名） |
| 注入 | 反射 `injectInputEvent(InputEvent, int)` |
| 定向到虚拟屏 | 反射 `InputEvent.setDisplayId(int)` —— 按键事件没有公开的 displayId 入口 |
| 模式常量 | 反射 `INJECT_INPUT_EVENT_MODE_ASYNC` —— **绝不硬编码**（依据见 §4） |
| 事件本体 | 全走**公开 API**：`MotionEvent.obtain` 的公开重载、`KeyEvent` 公开构造器 |

⇒ 运行期反射**恰好 3 个成员**。交叉验证：scrcpy 的 `wrappers/InputManager.java` 走的是同一条路。

---

## 2. 事件表示：**触点快照 + 粗粒度 action**

```json
{ "cmd": "input", "requestId": N,
  "payload": { "action": "down" | "move" | "up" | "cancel" | "scroll" | "keyDown" | "keyUp",
               "pointers": [ { "id": 0, "x": 640, "y": 360 } ],
               "scroll": { "dx": 0, "dy": -120, "x": 640, "y": 360 },
               "key": { "code": 4, "meta": 0 } } }
```

**确切 JSON 与各字段语义见 [`agent/transport-shapes.md`](transport-shapes.md) §5。**
（`action` / `pointers` / `scroll` / `key` 全在 **`payload`** 内；信封只带 `cmd` + `requestId`。）

**`pointers` = 变更后的当前触点集合** —— `down` 含刚按下的那根；**`up` 给"抬起之后剩下的"**
（可能为空 = 最后一根抬起）；`move` 给当前全部；`cancel` 忽略。

**`pointerIndex`（Android 的 `ACTION_POINTER_DOWN` / `ACTION_POINTER_UP` 与 `actionIndex`）由 agent 推导** ——
比较"上一份触点快照"即可得出；**被抬起的那根由 agent 从上一份快照补回事件数组**
（Android 要求它在数组里，坐标取它最后的位置）。
⇒ **PC 不需要懂 Android 的 action 细节**，协议与平台解耦 ✓
（代价：`input` 线程持有"上一份触点快照 + 手势 `downTime`"这点小状态 —— 单线程，无锁 ✓）

---

## 3. 坐标 = **屏像素（整数）**，以**当前屏尺寸**为准

PC 渲染的就是解码帧 ⇒ 帧尺寸 **=** 屏尺寸（[`agent/display.md`](display.md) 硬约束：编码尺寸必须 = 屏尺寸）
⇒ **PC 的鼠标坐标直接可用** ✓ 无浮点、无精度损失 ✓

⚠️ **尺寸变更后** PC 必须用**新尺寸**（`applyScreen` 的回报里有 `applied` 实际生效值）。

---

## 4. 注入模式 = **`ASYNC`（硬约束）**

| 模式 | 真机值 | 一次注入耗时（实测） |
|---|---|---|
| `INJECT_INPUT_EVENT_MODE_ASYNC` | **0** | **14 ms** |
| `INJECT_INPUT_EVENT_MODE_WAIT_FOR_RESULT` | **1** | —（不用） |
| `INJECT_INPUT_EVENT_MODE_WAIT_FOR_FINISH` | **2** | **10089 ms** ✗✗ |

`WAIT_FOR_FINISH` 会等事件被消费完 —— **目标屏没有窗口接收时会等满 10s 超时**，
会把 `input` 线程**锁死 10 秒** ⇒ **绝对禁用** ✓

⚠️ 三个值**取自反射**（`display-flags.md` §1「绝不硬编码」同样适用）：实测本机
`ASYNC=0 / WAIT_FOR_RESULT=1 / WAIT_FOR_FINISH=2`，而本节原表曾把 `WAIT_FOR_FINISH` 记成 `1` ✗
—— 硬编码就会写错。

---

## 5. 队列与合并 —— **实时性的核心**

| 项 | 决定 |
|---|---|
| 线程 | **每路一条 `input` 线程**（[`agent/runtime-threads.md`](runtime-threads.md) §2：**不与 `slow` 共用** —— 一次枚举（百毫秒级）就会让拖动卡住） |
| 队列 | 容量 **8**，**入队非阻塞** |
| **合并：`move`** | 队尾是 `move` ⇒ **覆盖**（"最新的移动覆盖还没处理的移动"） |
| **合并：`scroll`** | 队尾是 `scroll` ⇒ **累加 `dx` / `dy`** —— ⚠️ **绝不能覆盖**：滚动量是**增量**语义，覆盖会**少滚** |
| **丢弃** | 满了 ⇒ 丢**最旧的 `move` / `scroll`**；**`down` / `up` / `cancel` / `key` 绝不丢**（丢了状态就错乱） |
| **收益** | **最坏延迟 = 一次注入（~3 ms）** —— 与 PC 送多快**无关** ✓ |

⇒ **"高频"不是靠吞吐，是靠合并**：PC 送 1000 Hz 也好，`input` 线程永远只处理"**最新状态**" ✓ 跟手 ✓

⚠️ 「绝不丢」有**硬上界**兜底（不可合并事件累计 **64** 条）：注入线程万一被卡住
（例如系统输入分发停摆），`down` / `key` 这类事件理论上能无限堆积 ⇒ **内存无界**。
到上界时丢**最旧**的一条 —— 与整个队列"只剩最新状态"的哲学一致：要牺牲就牺牲最陈旧的。
正常路径永远碰不到它（PC 定频采样 ≤ 屏刷新率，而一次注入 ~3 ms）。

**实测参考**：7 个触摸事件（down + 5×move + up）**总耗时 23 ms** ⇒ 单次约 **3 ms** ✓

### 5.1 双端合并，职责不同（**两端都要做**）

| 端 | 作用 | 机制 |
|---|---|---|
| **PC** | 省**协议开销**（每秒上千条 JSON + CRC + socket 写） | **定频采样**（上限 = 屏刷新率）+ `scroll` 累加 —— 见 [`tech-stack/pc.md`](../tech-stack/pc.md) §4 |
| **agent**（本节） | **最后一道保险** —— PC 送多快都**不影响注入延迟** | 队列合并 + 容量 8 |

⚠️ **只做 agent 侧** ⇒ 协议开销**照付** ✗；**只做 PC 侧** ⇒ agent 被高频消息冲击（但因为有队列也扛得住）✗
⇒ **两端都要** ✓

---

## 6. 按键 = **Android `keyCode` + `metaState`**

最灵活（任意键都能送）、agent 侧**零映射代码**；PC 侧维护一张"键盘按键 → keyCode"表（反正要写）✓

---

## 7. 失败处理

**回报规则（高频流的省流约定）**：**只有失败才回 ack** —— 成功、被合并、被丢弃都**静默**。
输入是高频流（PC 定频采样，上限 = 屏刷新率），逐条回 ack 会把协议开销翻倍，而它没有信息量；
只有失败才需要 PC 知道 ✓

| 情况 | 分级（[`agent/runtime-failure.md`](runtime-failure.md)） | `error.code` |
|---|---|---|
| 单次注入返回 `false` / 抛异常 | **可恢复** —— 记日志 + 随 `requestId` 回报，不动状态 | `inputFailed` |
| 载荷与 `action` 对不上（缺载荷 / 触点变化矛盾） | **可恢复** —— PC 侧 bug | `badInput` |
| **注入被系统策略禁止**（能力缺失） | **子系统级** —— 标记 `input` 子系统不可用、拒绝后续输入事件并**首次上报** | `inputDenied` |

---

## 8. 无屏时

输入**需要屏** ⇒ 无屏时 `input` 指令**回报失败**（可恢复级，`error.code = noScreen`）
（[`agent/runtime-lifecycle.md`](runtime-lifecycle.md) §4 的"无屏 + 已连接合法"是给**枚举**用的）。

目标屏由 `link` **经队列**告诉 `input` 线程（`InputCmd.ScreenChanged`），
且在**回报 `applyScreen` 成功之前**发布 —— 否则 PC 一收到回报就发输入事件，会先到一步而被拒 ✗

---

## 9. 变更记录

- **2026-10-07 定稿**：输入注入落盘。**四项前提实测通过**（注入权限 / 定向到虚拟屏 / 本机屏不受影响 /
  触摸被消费）；**事件表示 = 触点快照 + 粗粒度 action**（`pointerIndex` 由 agent 推导）；
  **坐标 = 屏像素**；**注入模式 = `ASYNC`**（`WAIT_FOR_FINISH` 实测 **10089 ms**，**硬禁用**）；
  **队列容量 8 + 队尾同类型 `move`/`scroll` 覆盖 + `down`/`up`/`cancel` 绝不丢**
  ⇒ 最坏延迟 = 一次注入（~3 ms）；**按键 = Android `keyCode` + `metaState`**。
  顺带收口 [`agent/display-flags.md`](display-flags.md) §6.4 的四项 flag（`OWN_FOCUS` 原假设被推翻）。
- **2026-10-07 修订：`scroll` 的合并方式 + 双端合并**：**`scroll` 必须是「累加」而不是「覆盖」** ——
  滚动量是**增量**语义，覆盖会**少滚**（原表述错误，已改）；
  并明确**双端合并、职责不同**：**PC 定频采样**（省协议开销，见 [`tech-stack/pc.md`](../tech-stack/pc.md) §4）
  + **agent 队列合并**（最后一道保险）—— **两端都要做**。
- **2026-10-07 S4 实现（本片交付）**：按本规范落地并在实体机（Android 16 / SDK 36）验收 **31 项全过**。
  实现层面定稿 / 修正如下：
  - **取法定稿**（§1 末）：`getSystemService(INPUT_SERVICE)` **零反射** + 反射 **3 个成员**
    （`injectInputEvent` / `InputEvent.setDisplayId` / `ASYNC` 常量）；事件本体全公开 API；
    交叉验证 scrcpy 同路；
  - **§4 纠错**：真机值 `ASYNC=0 / WAIT_FOR_RESULT=1 / WAIT_FOR_FINISH=2`，原表把
    `WAIT_FOR_FINISH` 记成 `1` ✗ ⇒ 由反射取常量，**不硬编码**；
  - **§2 精确化**：`pointers` 明确为"**变更后**的当前触点集合"（`up` 给"抬起后剩下的"）；
    `action` 补 `keyDown` / `keyUp`；`scroll` 补位置 `x` / `y`（Android 的 `ACTION_SCROLL` 要用坐标做命中判定）；
  - **§5 补硬上界**：不可合并事件累计 64 条即丢最旧（防内存无界，正常路径碰不到）；
  - **§7 补回报规则**：成功 / 合并 / 丢弃**静默**，只有失败回 ack；补三个错误码
    `inputFailed` / `badInput` / `inputDenied`；
  - **§8 补时序**：目标屏在**回报成功之前**发布，避免"PC 收到回报立刻发事件"时被拒。
  验收关键证据：注入滑动 ⇒ 虚拟屏原始截图 **91.9%** 行变化、滚轮 **91.8%**；
  **本机屏 0.000%**（基线 0.000%）；BACK ⇒ 画面 **100%** 变化且该屏前台由
  `com.android.settings/.MiuiSettings` 变为**无前台**；`input` 线程断开后退出（无泄漏）；
  单测 **60 项全绿**，构建**零警告**。
