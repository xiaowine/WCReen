# `:agent` · 报文形状（F4）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/transport.md`](transport.md)（F4）的**形状**部分。
> **消息清单与字段语义**见 [`agent/transport-messages.md`](transport-messages.md)；
> 字节框架（类型 / 长度 / CRC / 字节序）见 [`agent/transport-protocol.md`](transport-protocol.md)。
> 本文件只放**确切 JSON** —— 此前这些形状散在消息清单里、且只有带 `…` 的草图，两边实现各猜一次。
> **变更记录记在本文件 §6。**

**状态**：**已定**（2026-10-07，含 S4 的 `input` 形状；**`0x20` 指令统一为「信封 + `payload`」**）。

---

## 1. 能力表（`0x13`）

```json
{ "requestId": 3,
  "codecs": [ { "name": "c2.qti.avc.encoder", "mime": "video/avc", "codec": "H.264",
                "hardware": true, "bitDepths": [8],
                "profiles": ["Baseline", "Main", "High"],
                "colorStandards": ["BT709", "BT601_NTSC", "BT601_PAL"],
                "colorRanges": ["LIMITED", "FULL"], "colorTransfers": ["SDR_VIDEO"],
                "width":  {"min": 128, "max": 8192, "step": 2},
                "height": {"min": 128, "max": 8192, "step": 2},
                "refreshRate": {"min": 1, "max": 480, "step": 1},
                "bitrate": {"min": 1, "max": 220000000, "step": 1},
                "maxInstances": 16 } ],
  "defaults": { "codec": "H.264", "bitDepth": 8, "profile": "High",
                "colorStandard": "BT709", "colorRange": "FULL", "colorTransfer": "SDR_VIDEO",
                "bitrateBpp": 0.1, "bitrateMode": "VBR", "refreshRate": 60,
                "iFrameInterval": 1, "showSystemDecorations": false, "alwaysUnlocked": true } }
```

**逐编码器**给能力（**没有**顶层 `ranges` / `formatGroups`）；枚举**有序**（回退规则是"取第一个支持的"）；
`hardware: false` 的编码器**不可用**（绝不退软编）；`maxInstances` 是系统**声明**值，不是实测值。
结构依据 [`agent/encode-probe.md`](encode-probe.md) §4。

---

## 2. 申请屏幕（`0x20` 的 `applyScreen`）

`params` 里**每个字段都可缺省**：填 `0` / 空串 = 未指定，由 agent 按设备推导；
`streamId = 0` 表示"建新屏"（非 0 表示重连时认领冻结的屏）。

```json
{ "cmd": "applyScreen", "requestId": 7,
  "payload": { "streamId": 0,
               "params": { "width": 1920, "height": 1080, "refreshRate": 60, "dpi": 320,
                           "bitrateBpp": 0.1, "bitrate": 0, "bitrateMode": "VBR",
                           "codec": "H.264", "bitDepth": 8, "profile": "High",
                           "colorStandard": "BT709", "colorRange": "FULL", "colorTransfer": "SDR_VIDEO",
                           "showSystemDecorations": false, "alwaysUnlocked": true, "screenName": "" } } }
```

---

## 3. 指令回报（`0x21`）

```json
{ "requestId": 7, "ok": true,
  "result": { "streamId": 1,
              "applied": { "streamId": 1, "width": 1920, "height": 1080, "refreshRate": 60,
                           "dpi": 320, "bitrate": 12441600, "bitrateMode": "VBR",
                           "codec": "H.264", "codecName": "c2.qti.avc.encoder", "hardware": true, "bitDepth": 8,
                           "profile": "High", "colorStandard": "BT709", "colorRange": "FULL",
                           "colorTransfer": "SDR_VIDEO", "showSystemDecorations": false,
                           "alwaysUnlocked": true, "ownDisplayGroup": false,
                           "screenName": "WCReen-1" } } }
{ "requestId": 7, "ok": false, "error": { … }, "applied": { …已生效的那部分… } }
```

⚠️ **`applied` 的两个位置各司其职，不重复发**：**成功**走 `result`（含 `streamId` + 全量生效值）；
**失败但部分生效**走**顶层** `applied`（此时没有 `result`）。

---

## 4. 错误对象（`error` 字段）

```json
{ "class": "fatal" | "subsystem" | "stream" | "recoverable", "code": "…", "message": "…" }
```

- `class` = **错误分级**（[`agent/runtime-failure.md`](runtime-failure.md) §1）；
- `code` = **稳定的机器可读标识**（见下表）—— PC 按它分支，**不能随手改字符串**；
- `message` = 给人看的可诊断说明（"做不到即明确失败并给出**可诊断原因**"）。

**已登记的 `code`**：

| `code` | 含义 | 典型 `class` |
|---|---|---|
| `versionMismatch` | 协议版本不匹配（回报双方版本后断开） | `stream` |
| `unsupportedCmd` | 该 `cmd` 不在本实现支持的集合内 | `stream` |
| `screenCreate` | 建屏被系统拒绝（资源类，可重试）；**参数不满足能力也在这一类** | `recoverable` |
| `screenVerify` | 建屏后**回读 flags 与请求不符**（平台没照做） | `fatal` |
| `noHwEncoder` | 没有可用的硬编编码器（绝不退软编） | `subsystem` |
| `encoderOpen` | 编码器 `configure` / `start` 失败 | `recoverable` |
| `encoderVerify` | 编码器参数回读不符 | `fatal` |
| `encoderFailed` | 运行中编码器抛 `CodecException`（⇒ 拆屏 + 主动上报 `screenReleased`） | `stream` |
| `noScreen` | 无屏时收到输入指令（[`agent/input.md`](input.md) §8）—— PC 建屏后重试即可 | `recoverable` |
| `badInput` | 输入载荷与 `action` 对不上（缺载荷 / 触点变化矛盾）—— PC 侧 bug | `recoverable` |
| `inputDenied` | 注入被系统策略禁止（缺 `INJECT_EVENTS`）⇒ 子系统级、重试无意义 | `subsystem` |
| `inputFailed` | 单次注入返回 `false` / 抛异常 | `recoverable` |

---

## 5. `input` 指令（F5）

```json
{ "cmd": "input", "requestId": 7,
  "payload": { "action": "down", "pointers": [ { "id": 0, "x": 640, "y": 360 } ] } }

{ "cmd": "input", "requestId": 8,
  "payload": { "action": "scroll", "scroll": { "dx": 0, "dy": -120, "x": 640, "y": 360 } } }

{ "cmd": "input", "requestId": 9,
  "payload": { "action": "keyDown", "key": { "code": 4, "meta": 0 } } }
```

| `action` | 必给的载荷 | 语义 |
|---|---|---|
| `down` / `move` / `up` / `cancel` | `pointers[]` | 触摸；`pointers` 是**变更后**的当前触点集合（`up` 给"抬起后剩下的"，可为空；`cancel` 忽略它） |
| `scroll` | `scroll{dx,dy,x,y}` | 滚轮；`dx` / `dy` 是**增量**（合并时累加，绝不覆盖），`x` / `y` 是滚轮位置 |
| `keyDown` / `keyUp` | `key{code,meta}` | 按键；`code` = Android `keyCode`，`meta` = `metaState` |

`pointers[]` 每项 = `{ "id", "x", "y" }` —— 屏像素**整数**（编码尺寸 = 屏尺寸 ⇒ PC 的鼠标坐标直接可用），
`id` 由 PC 分配、手势内稳定（agent 由它推导 `pointerIndex`）。

⚠️ **成功不回 ack** —— 成功 / 被合并 / 被丢弃都**静默**，只有失败才回
（`noScreen` / `badInput` / `inputDenied` / `inputFailed`，见 §4）。
合并规则与理由见 [`agent/input.md`](input.md) §2 / §5 / §7。

---

## 6. 变更记录

- **2026-10-07 指令信封化**：§2 / §5 的两条 `0x20` 形状包上 **`payload`** ——
  信封 = `cmd` + `requestId`，载荷 = `payload` 内的结构体（字段名固定 `payload`，
  `stopAgent` / `endSession` / `requestSyncFrame` 这类**无字段指令省略**它）；
  `0x21` 回报形状**不变**。理由与影响见 [`agent/transport-changelog.md`](transport-changelog.md)。
- **2026-10-07 定稿**：从 [`agent/transport-messages.md`](transport-messages.md) 整段提成独立文件
  （该文件在回填确切 JSON 后超出单文件上限）。收录四条**确切形状** —— 能力表 / `applyScreen` /
  指令回报 / 错误对象 —— 并登记 8 条 `error.code`。
  两条被两实现**独立猜过**的形状在此统一：能力表**逐编码器**给能力（无顶层 `ranges` / `formatGroups`）；
  `applied` **成功走 `result`、部分成功走顶层**，两处不重复发。
- **2026-10-07 补 `input`（S4）**：新增 §5「`input` 指令」的确切 JSON 与 `action` → 载荷对照表；
  §4 的错误码 **8 → 12**（新增 `noScreen` / `badInput` / `inputDenied` / `inputFailed`）。
  同时写死一条协议约定：**`input` 成功不回 ack**（成功 / 合并 / 丢弃都静默），只有失败才回 ——
  它是高频流，逐条回 ack 会把协议开销翻倍。
