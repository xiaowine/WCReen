# `:agent` · 消息清单（F4）变更记录

> 本文件是 [`agent/transport-messages.md`](transport-messages.md) 的**变更记录**，由该文件 §6 整段提成独立
> （该文件在补 `input` 字段与 `subsystem` 后正好卡在单文件上限）。
> 其余规范见 [`agent/transport.md`](transport.md)（F4）总览。

**状态**：随 [`agent/transport-messages.md`](transport-messages.md)。

---

## 变更记录

- **2026-10-07 指令（`0x20`）信封化**：指令从**扁平**改为**「信封 + 载荷结构体」** ——
  `{"cmd":"…","requestId":N,"payload":{…}}`；载荷字段名**固定 `payload`**，
  **无字段指令省略它**（`stopAgent` / `endSession` / `requestSyncFrame`）；**`0x21` 回报形状不动**；
  两侧都改为**两段式解析**（先解信封 `cmd` + `requestId`，再按 `cmd` 把 `payload` 解成载荷结构体）。
  载荷结构体**不再携带** `cmd` / `requestId`（`ApplyScreenMsg` / `InputMsg` ⇒ `*Args`）。
  **动的理由**（代码里已显形，非理论）：同一条 JSON 被**解两遍**（`ReaderLoop` 先解出 `cmd` + `requestId`、
  再把同一个 payload 解成 `ApplyScreenMsg` / `InputMsg`）；每条指令的 DTO 都要**重复携带** `cmd` + `requestId`；
  载荷**没有自己的边界** ⇒ 结构体无法独立复用 / 单测。**协议版本号仍为 `1`** ——
  这是**破坏性形状变更**，但项目处于**初始开发阶段**，两端**同步替换**即可，不靠版本号区分
  （⇒ 不存在「旧新混用得到干净的 `versionMismatch`」这类兜底）。形状见
  [`agent/transport-shapes.md`](transport-shapes.md) §2 / §5，字段语义见
  [`agent/transport-messages.md`](transport-messages.md) §2.3。
- **2026-10-07 定稿**：从各功能文档收拢全部消息 —— **9 个类型**、**7 条指令**（`cmd` 区分）；
  `applyScreen` 承载全部建屏参数、回报携带 **`applied`** 且**允许部分成功**；主动上报 3 种事件
  （`screenReleased` **只在 agent 主动拆屏时发**）；**`streamId` 只在 `applyScreen` 与其回报里出现**。
  **`input` 已由 F5 补齐**（[`input.md`](input.md) §2）；F8 倾向不产生新消息。
- **2026-10-07 形状提成独立文件（S2 实施后）**：回填确切 JSON 后本文件超限 ⇒ **报文形状**
  （能力表 / `applyScreen` / 指令回报 / 错误对象 + 8 条 `error.code`）整段提成
  [`agent/transport-shapes.md`](transport-shapes.md)；本文件只留字段语义与指针。
- **2026-10-07 补缺**：**新增 `setPolicy` 指令**（`N` + 「断开后立即释放」）—— 它必须能**在任意时刻下发**
  （典型用法是 PC 关闭前临时打开）⇒ **必须是指令而非握手字段** ⇒ 指令数 **7 → 8**。
- **2026-10-07 变更记录提成独立文件（S4 实施后）**：本文件补完 `input` 的字段与
  `subsystem` 后正好 200 行 ⇒ 变更记录整段提成本文件（与 `runtime-changelog.md` /
  `display-changelog.md` / `features-changelog.md` 同一做法）。
  **`input` 的两条协议约定**同步落定：字段 = `action` + `pointers[]` / `scroll` / `key`
  （确切 JSON 见 [`agent/transport-shapes.md`](transport-shapes.md) §5）；
  **成功不回 ack**（成功 / 合并 / 丢弃都静默，只有失败回）——
  详见 [`input.md`](input.md) §7 与 [`transport-shapes.md`](transport-shapes.md) §5。
  主动上报补一个字段：`subsystemUnavailable` 带 **`subsystem`**（子系统名）。
  顺带**修掉一处自相矛盾**：§3.4 的示例原带 `streamId`，与 §4 约定 1（`streamId` 只出现在
  `applyScreen` 与其回报里）冲突 ⇒ 去掉，并写明"主动上报不带 `streamId`，目标屏由连接本身确定"。
- **2026-10-07 上报补 `hardware`**：`applied` 增加 **`hardware`**（实际用的是硬编还是软编）；
  能力表的 `CodecMsg.hardware` 语义同步改为"**如实上报**（`false` = 软编回退）"——
  配合 [`agent/encode.md`](encode.md) 纪律 2 的放宽（优先硬编、无硬编退软编）。
  形状同步：[`agent/transport-shapes.md`](transport-shapes.md) §3 的示例补该字段。
- **2026-10-08 屏方向入协议**：`applied` 增加 **`rotation`**（建屏那一刻屏的方向，`0`/`1`/`2`/`3` =
  `Surface.ROTATION_0/90/180/270`，通常是 `0`），并新增主动上报 **`screenRotation`**（带 `rotation`，
  **方向变化**时发）。为什么两条都要：建屏时屏上还没有 App（值必为 0），方向要等 App 拉起后由它请求驱动
  ⇒ "变化"只能靠事件送。**不带 `streamId`**（§4 约定 1 不变：目标屏由连接本身确定）。
  动因：A15/14 上把屏设为可旋转（"任意分辨率铺满"）⇒ 帧内容相对帧缓冲转 90°，PC 必须跟转。
  形状同步：[`agent/transport-shapes.md`](transport-shapes.md) §3、
  [`agent/transport-messages.md`](transport-messages.md) §3.4。
- **2026-10-08 撤销上一条（产品决定：放弃"铺满"）**：`applied.rotation` 与主动上报 `screenRotation`
  **全部移除** —— 它们只服务于"屏可旋转 ⇒ 帧内容转 90° ⇒ PC 跟转"这条链路，而该链路已整体回退
  （见 [`agent/display-changelog.md`](display-changelog.md)）。**消息清单回到本文件上一条之前的状态**：
  `applied` 无 `rotation`、`event` 取值仍是 `screenReleased` / `subsystemUnavailable` / `fatal` 三个。
