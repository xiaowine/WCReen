# `:agent` · 消息清单（F4）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/transport.md`](transport.md)（F4）的**消息清单**部分。
> 字节格式见 [`agent/transport-protocol.md`](transport-protocol.md)；握手见 [`agent/transport-handshake.md`](transport-handshake.md)。
> **字段语义不在本文件重复** —— 只给名字与来源链接（[`agent/module.md`](module.md) §2 边界纪律）。
> **变更记录记在 [`agent/transport-changelog.md`](transport-changelog.md)。**

**状态**：**已定**（2026-10-07，含 S4 的 `input` 字段与 `subsystem`；**`0x20` 指令信封化**：破坏性形状变更，但**协议版本号仍为 `1`**，两端**同步替换**）。

---

## 1. 总览

| 类型 | 方向 | 消息 |
|---|---|---|
| `0x10` | PC→agent | 握手 |
| `0x11` | agent→PC | 握手应答 |
| `0x12` | PC→agent | 请求能力表 |
| `0x13` | agent→PC | 能力表 |
| `0x20` | PC→agent | **指令**（`cmd` 区分） |
| `0x21` | agent→PC | **指令回报** |
| `0x22` | agent→PC | **主动上报** |
| `0x01` | agent→PC | 视频帧 |
| `0x30` | 双向 | 心跳 |

---

## 2. PC → agent

### 2.1 握手（`0x10`）

```json
{ "version": 1 }
```

**精确匹配**；不匹配 ⇒ 回报双方版本并断开（[`agent/transport-handshake.md`](transport-handshake.md) §3）。

### 2.2 请求能力表（`0x12`）

```json
{ "requestId": 3 }
```

### 2.3 指令（`0x20`）

```json
{ "cmd": "applyScreen", "requestId": 7, "payload": { … } }
```

| `cmd` | 语义 | 载荷形状（`payload` 内） | 来源 |
|---|---|---|---|
| `applyScreen` | **申请屏幕**（建屏 / 重连认领 / 参数变更） | `streamId?` + `params{…}` | [`agent/display.md`](display.md) §2.0 |
| `endSession` | **结束会话**（拆屏但继续监听） | — | [`agent/runtime.md`](runtime.md) §2.1 |
| `stopAgent` | **停止 agent**（全局收尾） | — | 同上 |
| `enumerateApps` | 枚举可启动应用 | `forceRefresh?` | [`agent/launch.md`](launch.md) §3 |
| `launchApp` | 打开目标应用 | `component` | [`agent/launch.md`](launch.md) §4 |
| `requestSyncFrame` | 请求同步帧（主动刷新 / 丢包重同步） | — | [`agent/encode-lifecycle.md`](encode-lifecycle.md) §1.7 |
| **`setPolicy`** | **设定会话策略** | `nAccessFailures?` + `releaseOnDisconnect?` | [`agent/display-lifecycle.md`](display-lifecycle.md) §2 |
| `input` | 输入事件 | `action` + `pointers[]` / `scroll` / `key`；**成功不回 ack** | [`agent/input.md`](input.md) §2 |

> **载荷形状**（`payload` 内）的确切 JSON 见 [`agent/transport-shapes.md`](transport-shapes.md) §2（`applyScreen`）/ §5（`input`）。
> ⚠️ **`setPolicy` 可在任意时刻下发** —— 「断开后立即释放」的典型用法是 **PC 关闭前临时打开**
> （"我走了，把屏放掉"）⇒ **不能只在握手时给** ✓
> 它的两个字段语义见 [`agent/display-lifecycle.md`](display-lifecycle.md) §2：
> `nAccessFailures`（连续接入失败次数，缺省 5）与 `releaseOnDisconnect`（缺省关）。
> **两者都按路生效**（计数按路各一份，见 [`agent/transport-topology.md`](transport-topology.md) §4.1）。

**`applyScreen` 的 `params`** —— **全部可缺省**，缺省由 agent **按设备推导**，**实际生效值必须回报**：

| 组 | 字段 | 语义出处 |
|---|---|---|
| 画质 / 编码 | `width` `height` `refreshRate` `bitrateBpp`（或 `bitrate`）`bitrateMode` | [`agent/encode-params.md`](encode-params.md) §2 / §3 |
| 格式组 | `codec` `bitDepth` `profile` `colorAspect` `colorRange` `colorTransfer` | [`agent/encode-format.md`](encode-format.md) |
| 屏行为 | `showSystemDecorations` `alwaysUnlocked` | [`agent/display-flags.md`](display-flags.md) §2 / §4 |
| 其他 | `dpi` `screenName` | [`agent/display.md`](display.md) §2 |

> ⚠️ `OWN_DISPLAY_GROUP` **不暴露给 PC**（派生位，只回报结果）；
> `DESTROY_CONTENT_ON_REMOVAL` 与其余必须位**不可配**（[`agent/display-flags.md`](display-flags.md) §2 / §6.3）。
> **确切 JSON 见 [`agent/transport-shapes.md`](transport-shapes.md) §2。**

> `applied` 里回报的生效值包含 **`hardware`** —— 标记实际用的是**硬编还是软编**
> （`false` = 本机无硬编时的软编回退；能力表里每个编码器也带同名字段，
> 见 [`agent/encode.md`](encode.md) 纪律 2）。

### 2.4 心跳（`0x30`）

```json
{}
```

---

## 3. agent → PC

### 3.1 握手应答（`0x11`）

```json
{ "version": 1, "state": "ready" }
```

`state` ∈ `ready` / `noHwEncoder`（后者 ⇒ 拒绝后续 `applyScreen`，
见 [`agent/runtime-failure.md`](runtime-failure.md) §2.2）。

### 3.2 能力表（`0x13`）

```json
{ "requestId": 3, "codecs": [ … ], "defaults": { … } }
```

**逐编码器**给能力（**没有**顶层 `ranges` / `formatGroups`）；枚举**有序**（回退规则是"取第一个支持的"）。
结构依据 [`agent/encode-probe.md`](encode-probe.md) §4；**启动时构建一次**、全进程不变、**无 `forceRefresh`**。
**确切 JSON 见 [`agent/transport-shapes.md`](transport-shapes.md) §1。**

### 3.3 指令回报（`0x21`）

```json
{ "requestId": 7, "ok": true,  "result": { "streamId": 1, "applied": { … } } }
{ "requestId": 7, "ok": false, "error": { … }, "applied": { …已生效的那部分… } }
```

⚠️ **`applied` 的两个位置各司其职，不重复发**：**成功**走 `result`（含 `streamId` + 全量生效值）；
**失败但部分生效**走**顶层** `applied`（此时没有 `result`）。
**确切 JSON 见 [`agent/transport-shapes.md`](transport-shapes.md) §3。**

| `cmd` | `result` / `applied` |
|---|---|
| `applyScreen` | `{ "streamId": 2, "applied": { … } }` —— **每个参数的实际生效值** |
| `enumerateApps` | `{ "apps": [ { "package": …, "component": …, "label": … } ] }` |
| 其余 | 空 |

⚠️ **`applyScreen` 允许"部分成功"** —— 例如码率生效了但尺寸不能原地改。
⇒ 此时回报 **`ok: false` + `error` + 顶层 `applied`（已生效的那部分）**，
**不丢信息**（依据 [`agent/display.md`](display.md) §2.1 的三类变更处理）。

### 3.4 主动上报（`0x22`）

```json
{ "event": "screenReleased", "reason": "nAccessFailures" }
{ "event": "subsystemUnavailable", "subsystem": "input" }
{ "event": "fatal", "reason": "…" }
```

⚠️ **主动上报不带 `streamId`** —— 与 §4 约定 1 一致（`streamId` 只出现在 `applyScreen` 与其回报里）：
目标屏**由连接本身确定**（一条连接 = 一块屏）✓
`subsystemUnavailable` 的 `subsystem` 取值：`encoder` / `input`。

| `event` | 何时发 | 来源 |
|---|---|---|
| `screenReleased` | **agent 主动拆屏**时；`reason` ∈ `nAccessFailures` / `fatal` | [`agent/runtime-lifecycle.md`](runtime-lifecycle.md) §7 |
| `subsystemUnavailable` | **首次**判定某子系统能力缺失（之后相关指令各回报同一原因） | [`agent/runtime-failure.md`](runtime-failure.md) §3 |
| `fatal` | 进程即将收尾（**尽力**发；socket 已坏时只能靠退出码） | 同上 |

> ⚠️ **PC 自己发的 `endSession` 不发这个** —— 它的**回报**已覆盖，不重复
> （`screenReleased` 只用于"**agent 单方面改变的事实**"）。

### 3.5 视频帧（`0x01`）

**二进制**，见 [`agent/transport-protocol.md`](transport-protocol.md) §2。

### 3.6 心跳（`0x30`）

```json
{}
```

---

## 4. 三处结构约定

| # | 约定 | 理由 |
|---|---|---|
| 1 | **`streamId` 只出现在 `applyScreen` 与其回报里** | "一条连接 = 一块屏"（[`agent/launch.md`](launch.md) §2）⇒ 之后所有指令的目标屏**由连接本身确定**，不必带 |
| 2 | **`applyScreen` 允许部分成功**（`ok:false` + `applied`） | 参数分三类变更处理，必须能表达"改了一部分" |
| 3 | **`screenReleased` 只在 agent 主动拆屏时发** | PC 发的 `endSession` 由它的回报覆盖，不重复 |

### 4.1 错误对象

```json
{ "class": "…", "code": "…", "message": "…" }
```

`class` = 错误分级（[`agent/runtime-failure.md`](runtime-failure.md) §1）；
`code` = 稳定的机器可读标识（**已登记的 8 条见 [`agent/transport-shapes.md`](transport-shapes.md) §4**）；
`message` = 给人看的可诊断说明（"做不到即明确失败并给出**可诊断原因**"）。

---

## 5. 待补

| 来源 | 待补内容 |
|---|---|
| `enumerateApps` 的 `apps[]` | **S3（F2）** 实现时补齐 |
| `setPolicy` 的字段 | **S5** 实现时补齐 |

> **F8 已撤销**（2026-10-07）—— 它**不产生新消息**：参数集由 `applyScreen` 承载、调试开关是 CLI。

---

## 6. 变更记录

**已提成独立文件** ⇒ [`agent/transport-changelog.md`](transport-changelog.md)
（本文件补完 `input` 字段与 `subsystem` 后正好 200 行，按 [`agent/runtime-changelog.md`](runtime-changelog.md)
等的同一做法外置）。
