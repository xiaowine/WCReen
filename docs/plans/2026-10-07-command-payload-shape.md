# 协议：指令（`0x20`）改为「信封 + 载荷结构体」

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 定义的**实现方案文档**。
> 写完即冻结；实施中的偏差另建文档。
> 相关规范：[`agent/transport-protocol.md`](../spec/agent/transport-protocol.md)、
> [`agent/transport-messages.md`](../spec/agent/transport-messages.md)、
> [`agent/transport-shapes.md`](../spec/agent/transport-shapes.md)。

**状态**：**已批准**（2026-10-07，用户在讨论中选定：字段名 `payload`、无字段指令省略 `payload`）。

---

## 1. 背景与目标

`0x20` 指令现在是**扁平**的：指令自己的字段与 `cmd` / `requestId` **平铺在同一层**。

```json
{ "cmd": "applyScreen", "requestId": 7, "streamId": 0, "params": { … } }
{ "cmd": "input",       "requestId": 8, "action": "down", "pointers": [ … ] }
```

**代价已经在代码里显形**（不是理论问题）：

| 代价 | 证据 |
|---|---|
| 同一条 JSON 被**解两遍** | `agent/.../runtime/ReaderLoop.kt:251` 解出 `cmd`+`requestId`，`:304` / `:335` 再把**同一个 payload** 解成 `ApplyScreenMsg` / `InputMsg` |
| 每条指令的 DTO 都要**重复携带** `cmd` + `requestId` | `proto/Screen.kt::ApplyScreenMsg`、`proto/Input.kt::InputMsg` 都各带一份 |
| 载荷**没有自己的边界** ⇒ 结构体无法独立复用 / 单测 | 同上 |
| 分派器必须先知道是哪条指令、才知道该看哪些字段 | 扁平形状的固有特性 |

**目标**（验收标准）：

1. 线形状改为 **「信封 + 载荷结构体」**：`{"cmd": …, "requestId": …, "payload": { … }}`；
2. 两侧都变成**两段式解析**：先解信封（`cmd` / `requestId`），再按 `cmd` 把 `payload` 解成对应的**载荷结构体**；
3. **无字段指令省略 `payload`**（`stopAgent` / `endSession` / `requestSyncFrame`）；
4. `0x21` 回报形状**不变**（它的 `result` / `applied` 本来就是嵌套的）；
5. 既有行为一条不少：握手 / 能力表 / 建屏 / 帧路径 / 输入的**外部可观测行为不变**。

---

## 2. 现状调研（事实依据）

| 事实 | 依据 |
|---|---|
| 扁平形状是**白纸黑字**写进规范的 | `transport-protocol.md`（类型表：**指令** —— `{"cmd":"…","requestId":N,…}`）、`transport-messages.md` §2.3、`transport-shapes.md` §2 / §5 |
| 两侧 DTO 各带 `cmd` + `requestId` | `agent/proto/{Control,Screen,Input}.kt`；`pc/proto/src/dto.rs` |
| PC 侧用 serde **内部标签**枚举 ⇒ 字段必然平铺 | `dto.rs`：`#[serde(tag = "cmd")] pub enum Command { … }` |
| 版本号是**精确匹配**、两处常量 | `agent/transport/Handshake.kt::PROTOCOL_VERSION`、`pc/proto/src/dto.rs::PROTOCOL_VERSION`（现均为 `1`） |
| 收到不认识的类型 / 不匹配版本时两侧都已明确失败 | `ReaderLoop` 的握手与 `else` 分支；`main`/`session` 的错误路径 |
| `input` 的**成功静默**与 `applyScreen` 的**部分成功**都与本改动正交 | `input.md` §7、`display.md` §2.1 |

**两侧可用的机制**（已核对，无需新依赖）：

- **Rust / serde**：`#[serde(tag = "cmd", content = "payload")]`（相邻标签）**支持 newtype 变体**，
  再配一层 `#[serde(flatten)]` 的 `request_id` ⇒ 输出正好是
  `{"requestId":7,"cmd":"input","payload":{…}}`；无字段变体是 unit 变体 ⇒ 不带 `payload` ✓
- **Kotlin / Gson**：先解信封 `CommandMsg { cmd, requestId, payload: JsonObject? }`，
  再 `gson.fromJson(payload, XXXArgs::class.java)` ⇒ 第二次只解**小载荷** ✓

---

## 3. 方案与取舍

### 3.1 采纳：**信封 + `payload` 结构体**

```json
{ "cmd": "applyScreen", "requestId": 7, "payload": { "streamId": 0, "params": { … } } }
{ "cmd": "input",       "requestId": 8, "payload": { "action": "down", "pointers": [ … ] } }
{ "cmd": "keyDown" 等其它 action 同 input }
{ "cmd": "stopAgent",   "requestId": 9 }
```

| 决定 | 取值 | 理由 |
|---|---|---|
| 载荷字段名 | **`payload`** | 与协议里「帧负载」的用词一致；不与 `applyScreen` 内部的 `params` 撞名 |
| 无字段指令 | **省略 `payload`** | 消息最小；两侧把「字段不在」与「空对象」都当空载荷 |
| 回报 `0x21` | **不动** | 它的 `result` / `applied` 已经是嵌套的，改了没有收益 |
| 协议版本 | **`1` → `2`** | 线形状变了；精确匹配保证「旧 PC 收到干净的 `versionMismatch`」而不是诡异行为 |
| 载荷类型命名 | agent `proto/` 里改为 `*Args`（`ApplyScreenArgs` / `InputArgs` …） | 它们不再是「消息」（不含信封），名字要跟着语义走 |

### 3.2 被否方案

| 方案 | 否掉原因 |
|---|---|
| 保持扁平 | 用户指出且证据充分（同包解两遍、DTO 重复携带信封字段） |
| 载荷字段按指令**各自命名**（`"input"` / `"screen"`…） | 分派器要多一张 `cmd` → 字段名 的表；自描述性收益不抵额外约定 |
| 只给**多字段指令**加结构体，单字段的保持扁平 | 两种形状并存 ⇒ 解析器要写两套规则，正是本次要消除的问题 |
| 顺手把 `0x21` 回报也重排 | 它本来就嵌套；「不改不需要改的」 |

---

## 4. 影响面

| 区域 | 变化 |
|---|---|
| `agent` `proto/Control.kt` | `CommandMsg` → 信封（`cmd` + `requestId` + `payload: JsonObject?`）；`Cmd` 常量不变 |
| `agent` `proto/Screen.kt` / `Input.kt` | `ApplyScreenMsg` → `ApplyScreenArgs`、`InputMsg` → `InputArgs`（去掉 `cmd` / `requestId` 字段） |
| `agent` `runtime/ReaderLoop.kt` | 分派改为「解信封 → 按 `cmd` 解载荷」；`handleApplyScreen` / `handleInput` 只解载荷 |
| `agent` `transport/Handshake.kt` | `PROTOCOL_VERSION` → `2` |
| `agent` 测试 | `WireTest` / `CodecTest` 里涉及指令形状的用例同步 |
| `pc` `proto/src/dto.rs` | `Command` → `{ request_id, #[serde(flatten)] body }` + `CommandBody`（相邻标签）；载荷结构体独立 |
| `pc` `app/src/session.rs` | 构造指令处改按新结构（`bring_up` / `send_stop_agent` / 输入发送） |
| `pc` 测试 | `protocol.rs` 的两条 `input` 契约测试 + `applyScreen` 往返用例同步为新形状 |
| 文档 | `transport-protocol.md`（类型表 + §3.2）、`transport-messages.md` §2.3/§5、`transport-shapes.md` §2/§5、`transport-changelog.md`、必要时 `agent/code.md`（DTO 形态） |
| **部署** | 两侧必须**一起换**：设备上的 jar 重推 + PC 二进制重建 ✓（旧新混用时是干净的版本不匹配） |
| 对外契约 | **协议版本号变更**（1 → 2）；CLI / 部署路径 / 目录布局不变 |

---

## 5. 实施步骤

1. **agent 侧**：改 `Control.kt` / `Screen.kt` / `Input.kt` / `ReaderLoop.kt` + `Handshake.PROTOCOL_VERSION = 2`；
2. **agent 测试**：同步形状相关用例，跑 `:agent:testDebugUnitTest` + `assembleRelease`（零警告）；
3. **pc 侧**：改 `dto.rs`（信封 + 载荷结构体）与 `session.rs` 的构造点；
4. **pc 测试**：同步 `protocol.rs` 的契约用例，跑 `cargo test --workspace`（全绿、零警告）；
5. **文档**：按 §4 的清单回填（含 `transport-changelog.md` 一条）；
6. **实机**：重推 jar + 重启 agent + 重启 PC 客户端，跑一遍既有验收路径。

---

## 6. 验证方式

| # | 场景 | 期望 |
|---|---|---|
| 1 | 两侧单测 | agent 全绿、`cargo test --workspace` 全绿、两侧构建零警告 |
| 2 | **实机握手 + 建屏** | 握手回报版本 `2`；`applyScreen` 回报 `applied` 与改动前逐字段一致 |
| 3 | **实机输入**（既有验收） | 真实拖动 ⇒ PC 侧计数增长、虚拟屏内容变化；Escape ⇒ 该屏前台清空 |
| 4 | **形状自证** | 抓一条真实指令的 JSON（日志或临时打印），确认是 `{"cmd":…,"requestId":…,"payload":{…}}`，且无字段指令不带 `payload` |
| 5 | 版本不匹配 | 用旧版 PC 连新版 agent ⇒ 干净地报 `versionMismatch` 并断开（不产生诡异行为） |

---

## 7. 风险与回滚

| 风险 | 应对 |
|---|---|
| 过渡期两侧不同步 ⇒ 连不上 | 这正是精确匹配的**设计行为**：报版本不匹配、不产生诡异行为；两侧一起换即可 |
| serde 相邻标签 + `flatten` 的组合出坑 | 已核对机制成立；单测里**钉死确切 JSON**（不是只测往返） |
| Kotlin 侧 `JsonObject` 载荷的 keep 规则 | 载荷类型仍全在 `proto` 包内 ⇒ 既有 `-keep class …proto.**` 覆盖 ✓ |
| 用户正在测试中被中断 | 本改动**排在在途修复之后**、由用户决定何时切换（见 §8） |
| 回滚 | 两侧各改回一版 + 版本号回 `1`；改动集中在 DTO / 解析 / 文档，无数据迁移 |

---

## 8. 未决问题

| # | 问题 | 影响 |
|---|---|---|
| 1 | **何时切换**：这是一次**破坏性协议变更**（设备 jar 要重推、PC 要重建）⇒ 是否等当前测试告一段落再做 | 期间旧版本继续可用；由用户决定 |

---

## 9. 变更记录

- **2026-10-07 起草并批准**：用户提出扁平形状解析不便；证据（同包解两遍、DTO 重复携带信封字段）成立 ⇒
  改为「信封 + `payload` 结构体」，无字段指令省略 `payload`，`0x21` 不动，协议版本 `1 → 2`。
  否掉「保持扁平」「按指令各自命名载荷」「只给多字段指令加结构体」「顺手重排回报」四条。
  **实施时机留待用户决定**（§8 #1）—— 会中断正在进行的实机测试。
- **2026-10-07 实施中改令（用户，优先于上文）**：**协议版本号保持 `1`，不升版** ——
  项目仍在**初始开发阶段**，形状变更直接**破坏性**、两端**同步替换**即可，不靠版本号区分。
  ⇒ §3.1 与 §4 里「协议版本 `1` → `2`」两处**作废**；§6 验证表第 5 行
  （「用旧版 PC 连新版 agent ⇒ 干净地报 `versionMismatch`」）**随之作废**（不再成立）。
  §1 目标与 §3.1 的其余部分（信封 + `payload`、无字段省略、`0x21` 不动、载荷类改名）**不变**。
- **2026-10-07 交付并实机验收**：两侧同步换完（**版本号仍为 `1`** ✓）。
  - **agent**：`CommandMsg` 改为信封（`cmd` + `requestId` + `payload: JsonObject?`）；
    `ApplyScreenMsg`→`ApplyScreenArgs`、`InputMsg`→`InputArgs`（不再带信封字段）；
    新增 `Codec.decodePayload`（第二段解析的单一入口，`null` ⇒ 空载荷、坏 JSON ⇒ `BAD_JSON`）；
    `ReaderLoop` 两段式分派 ⇒ **「同包解两遍」消失**。连带改 `InputCmd` / `InputQueue` / `InputLoop` /
    `EventBuilder`（`requestId` 从载荷提到事件上，滚轮合并仍取最新 id，行为不变）。
    单测新增两条形状断言（有字段的帧含 `"payload":{`、`stopAgent` **不含**）+ 一条「载荷缺省 ⇒ 空载荷」。
  - **pc**：`Command { request_id, #[serde(flatten)] body }` + `#[serde(tag="cmd", content="payload")]`
    的 `CommandBody`；`ApplyScreenArgs` / `InputArgs` 独立成结构体；无字段指令用 **unit 变体**
    ⇒ 序列化时确实**不带 `payload`**（**断言**而非假设）。协议契约测试改成断言**确切 JSON 字符串**。
  - **验证**：agent `assembleRelease` **零警告**、单测全绿；`cargo build --release --workspace`
    **零警告**、`cargo test --workspace` 全绿（app 21 + protocol 23）。
    **实机**：重推 jar + 重启 agent + 用新二进制重建 PC ⇒ 握手 / 能力表 / `applyScreen` 全过
    （虚拟屏 `displayId=122`、屏名 `WCReen-1`，按路派生 ✓）；**真实鼠标拖动 + 按键**
    ⇒ PC 侧 `输入 19 条（被拒 0）`、虚拟屏原始截图 **85.6%** 行变化、解码 0 → 382 帧、
    硬解回读 `nv12` ✓。
  - **边界**：`0x21` 回报、CRC / 字节序、版本精确匹配逻辑**均未动**；`rules.keep` 的
    `-keep class …proto.**` 仍覆盖全部新类（`JsonObject` 是 Gson 类型，无需 keep）。
