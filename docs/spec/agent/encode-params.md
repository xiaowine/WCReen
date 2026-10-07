# `:agent` · 编码参数（F3）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/encode.md`](encode.md)（F3）的**编码参数**部分。
> 能力探测见 [`agent/encode-probe.md`](encode-probe.md)；格式组见 [`agent/encode-format.md`](encode-format.md)；
> 重建 / 解冻 / 参数变更见 [`agent/encode-lifecycle.md`](encode-lifecycle.md)。

---

## 1. 低延迟（已定）

**决定：低延迟一律开启，不作为可配参数。**

实现：编码器 `start()` 后立即 `setParameters({PARAMETER_KEY_LOW_LATENCY: 1})`，**不提供关闭开关**。

**实测依据（§1.1）**：该参数**被接受但无可观测效果** —— 本机 QTI 硬件编码器**默认已在低延迟状态**。
保留它是因为**零成本、且换设备可能有效**；但**不得把延迟目标寄托在它上面**。

### 1.1 实测数据

| 负载 | 条件 | 中位延迟 | p90 |
|---|---|---|---|
| 1280×720@60 | 未调用 / `LOW_LATENCY=1` / `=0` | 13.8 / 13.3 / 13.5 ms | 16.9 / 16.2 / 16.5 ms |
| **1440×3200@60** | 未调用 / `LOW_LATENCY=1` / `KEY_LATENCY=1` / `=0`（后两者为 configure 期**灵敏性对照**） | 21.7 / 22.0 / 21.6 / 22.0 ms | 24.9 / 24.3 / — / — |

**灵敏性对照的意义**：`KEY_LATENCY` 的 0/1 在**输出格式里如实回读**（`latency=0` / `latency=1`）
⇒ 参数确实生效了，**只是不改变延迟** ⇒ 结论不是"没测出来"，而是"本来就没什么可降的"。

### 1.2 延迟基线（供 F4 做端到端预算）

1440×3200@60 硬件编码：**最小 ~10.5 ms / 中位 ~22 ms / p90 ~25 ms**（≈1.3 帧周期），
且延迟**不随时间增长** ⇒ 该分辨率下编码器**能持续跟上 60fps**。

> 这是**编码器内部**的延迟，**不含**显示合成、传输与 PC 解码渲染 —— 端到端预算属 F4。

---

## 2. 码率

### 2.1 码率值：由 PC 二选一表达

**⚠️ 前提事实（已实测）：`KEY_BIT_RATE` 是必填的** —— 不设它 `configure()` 直接抛
`IllegalArgumentException`（消息为空），无论是否设了码率模式。
⇒ **不存在"不设码率、交给编码器自己算"这条路** ⇒ PC 什么都不给时，**agent 必须算出一个值**。

**两个字段在协议上并存、互不排斥** —— **协议不做互斥校验**；
**"二选一"只在 PC 的 UI 层保证**（界面上让用户选一种，避免困惑）。

| 方式 | 请求给什么 | agent 的行为 |
|---|---|---|
| **A. 按系数** | **bpp 系数** | 算 `码率 = 宽 × 高 × 刷新率 × 系数`（**刷新率**取自建屏参数，见 [`agent/display.md`](display.md) §2） |
| **B. 固定码率** | 码率绝对值 | 直接用 |

#### agent **保底**：优先级

```
系数  >  固定码率
```

| 收到的情况 | agent 的行为 |
|---|---|
| **只给系数** | 按系数算 |
| **只给固定码率** | 直接用 |
| **两个都给**（UI 上不该发生，但协议允许） | **按系数算**，忽略固定码率，并**回报"检测到两个值、以系数为准"** |
| **都不给** | 用**默认 bpp 系数**推导（该系数是**策略常量**，与"绝不硬编码平台值"不冲突） |

**默认 bpp 系数 = `0.1`**（**策略常量，PC 可覆盖**）⇒ 本机 1440×3200@60 ≈ **27.6 Mbps**、
1920×1080@60 ≈ 12.4 Mbps、1280×720@60 ≈ 5.5 Mbps、1920×1080@30 ≈ 6.2 Mbps。理由：

- 落在视频经验的 **0.05–0.15 bpp** 区间**下半**（屏幕内容比视频省）；
- 比原型用的 8 Mbps（= 0.029 bpp）**高一倍多** ⇒ 给滚动 / 动画留裕量；
- **`VBR` 下这是"平均目标"** ⇒ 静止屏时实际码率**接近 0**，不会真的常驻 27.6 Mbps；
- 实测 `getBitrateRange()` = `[1, 220 Mbps]` ⇒ **完全在范围内**（见 [`agent/encode-probe.md`](encode-probe.md) §1）。

> **为什么协议层不做互斥校验**：两个字段各自独立可选，**解析与装包最简单**；把"只能有一个"编码进协议，
> 要多一条校验 + 多一类错误码，**收益为零**。
> **为什么 agent 不报错**：缺省与容错是 agent 的职责（链路是「校验 → 应用 → **回报**」）；报错会让一次本可
> 正常建立的会话失败，而**静默取一个**又违反"必须回报" ⇒ 取「**按优先级取值 + 明确回报**」。

**与模式无关** —— `bitrate-mode` 是**独立的参数**（见 §2.3）；本节只定"**码率值怎么来**"。
两者都走通用链路「**校验 → 应用 → 回报实际生效值**」（[`agent/display.md`](display.md) §2）：agent 必须按编码器能力 `VideoCapabilities.getBitrateRange()` **夹到合法范围**，并**回报实际值**。

### 2.2 agent **不自主改码率**（已定）

**决定：码率只由 PC 决定；agent 不做任何自动 / 自适应降码率。**

- 变更**走重建编码器**（**不存在热改**，见 [`agent/encode-lifecycle.md`](encode-lifecycle.md) §2）——
  不再使用 `PARAMETER_KEY_VIDEO_BITRATE`；
- agent **只在收到 PC 指令时**才改，**不因拥塞 / 发热 / 路数变化自行调整**。

> ⚠️ **推论（留给 F4）**：agent 不会自救 ⇒ **拥塞信号必须由 F4 送回 PC**，
> 由 PC 决定降码率还是减路；否则链路只会一直堵着。

### 2.3 码率模式（**独立参数**，已定）

**`bitrate-mode` 是独立于"码率怎么表达"的一个参数**（PC 可选，缺省 **VBR**）。

| 模式 | 常量 | 语义 | 对 PC 的影响 |
|---|---|---|---|
| **VBR** | `1`（**缺省**） | `bitrate` 是**目标平均码率**，瞬时可超可低 | 画质稳，但**带宽有峰值**（可能达均值 1.5–2×） |
| **CBR** | `2` | `bitrate` 是**恒定目标**，编码器填充 / 丢帧维持 | **带宽平稳**，画质可能被牺牲 |

**实测：只支持 VBR 与 CBR** —— `CQ` 不被支持（显式设会抛异常），
见 [`agent/encode-pitfalls.md`](encode-pitfalls.md) §1 第 7 条。

⇒ **语义上模式与码率值独立**：PC 可以"给系数 + 要 CBR"，也可以"给固定码率 + 用缺省 VBR"。
但**效果耦合**：同一个码率值在 VBR 与 CBR 下的**带宽形态完全不同**
⇒ PC 做带宽预算时必须**同时看这两个参数**。

---

## 3. 帧率（已定）

**帧率 = 虚拟屏刷新率** —— 零拷贝下 agent **碰不到帧、无法丢帧**，
所以帧率只能由**建屏时的刷新率**决定（[`agent/display.md`](display.md) §2：PC 选、缺省 60、会话内固定）。

**编码器的 `KEY_FRAME_RATE` 必须同步刷新率**（已定）。

> 它**不控制实际出帧**（实际出帧由屏驱动），只影响**码率控制的分配**；
> 不同步会让码率控制按错误的帧率去分配比特。

---

## 4. 关键帧间隔（已定）

**默认 1 秒**（`KEY_I_FRAME_INTERVAL = 1`）。

- **不能长**：间隔 = 丢包重同步 / 新客户端接入 / 解冻后的**恢复时间**；
- **不能短**：I 帧远大于 P 帧 ⇒ 间隔越短**平均码率越高**；
- **怎么改**：运行期**没有**改间隔的参数 ⇒ 变更 = **重建编码器**；临时要 IDR 用 `PARAMETER_KEY_REQUEST_SYNC_FRAME`（按需）。

⚠️ **与"静止无帧"耦合**：屏静止时根本没有帧，"等下一个 IDR"**可能永远等不到** —— 见 [`agent/encode-lifecycle.md`](encode-lifecycle.md) §1。

**`PREPEND_HEADER_TO_SYNC_FRAMES = 1` 固定开启、不可更改**（已定）。

**实测验证**（1280×720@60，编码 3 秒，I 帧间隔 1s ⇒ 3 个 IDR）：

| 设置 | SPS / PPS 出现次数 | **IDR 前紧跟 SPS+PPS** |
|---|---|---|
| **1** | **4 / 4** | **3 / 3** ✓ |
| `0`（对照） | 1 / 1 | **1 / 3** ✗（后两个 IDR 前面是 P 帧） |

⇒ **每个 IDR 都自带参数集** ⇒ **新客户端拿到任意 IDR 即可解码**，
**F4 不需要单独的 CSD 交换环节**，协议里也不需要这个字段（它是固定的）。

**暂不做 PC 可配**（固定 1 秒）—— 后续需要再加。

---

## 5. 多路（已定）

**不设总量上限** —— **每路独立设置**；agent 侧**不拒绝、不偷改已有路**，也不做任何容量判断。

> 容量上限由"**实际能开几路**"自然形成（见 [`agent/encode-probe.md`](encode-probe.md) §3 与
> [`agent/encode-capability.md`](encode-capability.md) §4.1）；**PC 侧不做任何容量处理**
> （[`tech-stack/pc.md`](../tech-stack/pc.md) §3）⇒ 容量不足表现为**每路 fps 下降**，不拒绝、不降级。

> 另两条也已定：**码率表达方式与优先级**（§2.1）、**码率模式**（§2.3，独立参数、缺省 VBR）。

---

## 6. 我们**显式设置**的编码器参数（**总表**）

> **通则**：见 [`workflow.md`](../workflow.md) §4「**不依赖默认值**」（**全局规则**）——
> 下面每一行都**显式写出取值**并**回读确认**。

| 项 | 编码器默认 | **我们显式设成** | 备注 |
|---|---|---|---|
| `width` / `height` | — | 屏尺寸（**必须相等**） | [`agent/encode-probe.md`](encode-probe.md) §5 |
| `color-format` | — | `COLOR_FormatSurface` | 零拷贝前提 |
| `bitrate` | — | 由 PC 二选一推导 | **必填**，见 §2.1 |
| `bitrate-mode` | `1`(VBR) | **PC 选，缺省 `VBR`** | §2.3 |
| `frame-rate` | — | **= 刷新率** | §3 |
| `i-frame-interval` | — | **`1`**（秒） | §4 |
| `prepend-sps-pps-to-idr-frames` | `0` | **`1`**（**走运行期 `setParameters`**，不进 format） | §4；**不可更改**。⚠️ 某些编码器（实测：Android 15 模拟器软编）**拒绝**该键出现在 `format` 里（`configure` 抛 `IllegalArgumentException`，消息为空）⇒ 提示键一律走 `setParameters`；若编码器仍不生效，agent 会把编码器单独给的参数集（CSD）**补到首个关键帧**上（实现见 `encode/VideoEncoder.kt` 的 `poll()`：`BUFFER_FLAG_CODEC_CONFIG` 分支与首个关键帧的合并） |
| `profile` | `8`(High) | **`High`**（H.264）/ `Main`（HEVC） | [`agent/encode-format.md`](encode-format.md) §1 |
| `color-standard` | 报 `1`，**实际 BT.601** | **`BT709`**（缺省） | [`agent/encode-format.md`](encode-format.md) §2 |
| `color-range` | `2`(LIMITED) | **`FULL`**（缺省） | 同上 |
| `color-transfer` | `3`(SDR_VIDEO) | **`SDR_VIDEO`** | 同上 |
| **`max-bframes`** | **`0`**（**默认就不允许 B 帧**） | **`0`**（**显式化**） | B 帧会引入重排序延迟，与低延迟目标冲突。官方：*"The default value is 0, which means that no B frames are allowed."* |
| **`priority`** | `1`（non-realtime / best effort） | **`0`（realtime）** | 官方：*"0: realtime priority — meaning that the codec **shall** support the given performance configuration (e.g. framerate) at realtime … should only be used by media playback, capture, and possibly **realtime communication** scenarios"*。我们是实时投屏 ⇒ 设 0。**是 hint，不保证性能** |
| **`operating-rate`** | — | **不设** | 官方：专为"格式里的播放率 ≠ 实际采集率"（如慢动作 30fps 格式 / 240fps 采集）。**我们两者相等 ⇒ 不需要** |
| `latency` | `4` | **不设** | 实测设 `0`/`1`/不设**对延迟无影响**（§1），故不折腾 |

**运行时参数（按需，不算"参数变更"）**：`PARAMETER_KEY_REQUEST_SYNC_FRAME`（要 IDR，见 [`agent/encode-lifecycle.md`](encode-lifecycle.md) §1）；`PARAMETER_KEY_LOW_LATENCY`（**固定开**，见 §1）。
**不再使用**：`PARAMETER_KEY_VIDEO_BITRATE`（**已撤销热改**，见 [`agent/encode-lifecycle.md`](encode-lifecycle.md) §2）。

---

## 7. 变更记录

- **2026-10-06 初版**：低延迟、码率、帧率、关键帧间隔、多路、显式设置总表。
- **2026-10-06 决定 + 实测**：**低延迟一律开启、不作可配参数**（实测为 no-op，并记录延迟基线）；
  **码率表达与模式拆开**（协议不互斥 + UI 互斥 + agent 保底「系数 > 固定码率」；
  模式为独立参数、缺省 VBR）；**帧率 = 刷新率**（`KEY_FRAME_RATE` 同步）；
  **关键帧间隔默认 1s**、`PREPEND_HEADER_TO_SYNC_FRAMES` **固定 1**（实测每个 IDR 自带 SPS/PPS）；
  **多路暂不设上限**；**默认 bpp 系数 = 0.1**。
- **2026-10-06 收尾**：`max-bframes = 0`、`priority = 0`、`operating-rate` 不设、
  `latency` 不设；**§6 重写为「显式设置总表」**（落实"不依赖默认值"全局规则）。
- **2026-10-06 拆分**：本文件从 [`agent/encode.md`](encode.md) 的 §3、§4、§5.2 **整段提成独立文件**
  （原文件超单文件上限）；§ 号重排，跨文件引用改指新文件。
