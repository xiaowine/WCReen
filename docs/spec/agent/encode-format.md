# `:agent` · 格式组（F3）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/encode.md`](encode.md)（F3）的**格式组**部分。
> 能力探测见 [`agent/encode-probe.md`](encode-probe.md)；编码参数见 [`agent/encode-params.md`](encode-params.md)；
> **本机实测数值**见 [`agent/encode-capability.md`](encode-capability.md)。

---

## 1. 格式组的选择规则（已定）

**统一规则**：每个格式参数都**优先一个默认值**；**本机不支持时，退到「第一个支持的」**；
**编码器本身优先硬编、没有硬编才退软编**（软编尺寸/帧率上限低得多，见
[`agent/encode-capability.md`](encode-capability.md) §1.2），且退软编必须**如实上报**。

| 参数 | 优先默认 | 本机不支持时 |
|---|---|---|
| **codec** | **`H.264`** | 取**第一个支持硬解**的编码器 |
| **位深** | **8-bit** | 取第一个支持的 |
| **profile** | H.264 → **`High`**；HEVC → **`Main`** | 取第一个支持的 |
| **色彩**（standard / range / transfer） | `BT709 + FULL + SDR_VIDEO`（依据见 §2） | 取第一个支持的 |

- "支持"的判定用 `MediaCodecInfo`：`isHardwareAccelerated()` / `profileLevels` /
  `getVideoCapabilities()`（见 [`agent/encode-probe.md`](encode-probe.md) §1）；
- **PC 仍可在「申请屏幕」指令里指定**这些字段（**格式组**，见 [`agent/display.md`](display.md) §2.0）——
  本规则只定**缺省**；**实际生效值必须回读确认并回报**
  （**全局规则**，见 [`workflow.md`](../workflow.md) §4）。
- **每个参数的缺省取值必须出现在能力表里**（[`agent/encode-probe.md`](encode-probe.md) §4）——
  否则 PC 无从知道"缺省是什么"，只能等回报，等于没法提前决策。
- 实测本机：`c2.qti.avc.encoder`（H.264 HW，profile 有 Baseline / Main / **High**）
  ⇒ 取它 + **8-bit** + **High**。

**我们显式设成什么**，见 [`agent/encode-params.md`](encode-params.md) §6 的总表。

---

## 2. 色彩元数据（**已实测 + 已定**）

**测法**：喂 8 个**已知纯色**（红/绿/蓝/白/黑/灰128/黄/青）→ 编码 → 用 **ffprobe 读 SPS VUI**
→ 解码成 **YUV** 读 Y 值**反推编码器内部实际用的矩阵**（BT.601 与 BT.709 对纯色的 Y 差很大，
如纯红 Y=81 vs 63，判据不含歧义）。

### 2.1 结论 1：`MediaFormat` 报的色彩字段**不可信**

| 观测点 | 报的值 | 真值 |
|---|---|---|
| `getInputFormat().color-standard` | **1 = BT709** | **实际是 BT.601**（见 §2.2） |
| `getOutputFormat().color-standard`（**不设**时） | `2` = BT601_PAL | 码流 VUI 写 `bt470bg`（BT.601 系） |
| `getOutputFormat().color-standard`（**显式设**时） | 跟随请求 | 一致 ✓ |

⇒ 输入格式里那个声明是**"未初始化默认值"，不是真值** —— **不得据它做判断**。

### 2.2 结论 2：码流（SPS VUI）是自洽的 —— 这才是真值

| 配置 | VUI 声明 | **实测内部矩阵** | 自洽 |
|---|---|---|---|
| **不设** | `bt470bg` / `smpte170m` / `bt470bg`，range=`tv` | **BT.601 limited** | ✅ |
| 显式 BT709 + LIMITED | `bt709` ×3，`tv` | **BT.709 limited** | ✅ |
| 显式 BT601_NTSC + LIMITED | `bt470bg`/`smpte170m`/`smpte170m`，`tv` | **BT.601 limited** | ✅ |
| **显式 BT709 + FULL** | `bt709` ×3，**`pc`** | **BT.709 full** | ✅ **往返偏差 0** |
| 显式 BT601_NTSC + FULL | `bt470bg`/`smpte170m`/`smpte170m`，**`pc`** | **BT.601 full** | ✅ **往返偏差 0** |

**像素级往返**：8 个纯色解码后与输入**最大偏差 ≤ 2/255**（FULL 组为 **0**）
⇒ **只要按码流声明转换，颜色就是对的**。
且 `KEY_COLOR_STANDARD` / `KEY_COLOR_RANGE` 会**同时改变内部矩阵 / 范围与 VUI** ⇒ 两边始终一致。

**⇒ `COLOR_RANGE_FULL` 完全生效**（VUI 写 `pc`、内部转换也用 full-range）⇒ **可以用**。

### 2.3 结论 3：`color_range` 跟随我们显式设置的值

- **不设** ⇒ 编码器默认给 `LIMITED`（码流里 `tv`）；8 个纯色的 Y 落在 16–235；
- **显式设 `FULL`** ⇒ 码流里 `pc`，且**内部转换同步切到 full-range**（实测偏差 0）；
- ⇒ 两边始终一致，**`FULL` 可用**。

### 2.4 结论 / 规则

1. **色彩字段是 PC 可选的**（属「申请屏幕」指令的"格式"组，见 [`agent/display.md`](display.md) §2.0）——
   `color-standard` / `color-range` / `color-transfer` 都由 **PC 选择**，**不是钉死值**；
2. **agent 一律显式设置**，**绝不依赖设备默认** —— 实测设备默认是 BT.601，
   且 `MediaFormat` 报的值与真值不符（§2.1）；
3. **缺省按 §1 的通用规则推导**（优先默认 → 本机不支持则退到第一个支持的）。
   **优先值 = `BT709 + FULL + SDR_VIDEO`**，**依据**（不是随手选的常量）：
   - **虚拟屏是 SDR 的** —— `VirtualDisplayConfig` 没有色彩模式设置，虚拟屏不带 HDR 标志；
   - **内容是 sRGB** —— Android UI 默认；BT.709 与 sRGB 的 primaries **相同**；
   - **range 取 `FULL`** —— 源头就是 sRGB **0–255**，`FULL` **不浪费码值**（见下表的取舍）；
     实测 VUI 与内部转换都正确写 / 用 `pc`，往返偏差 **0**；
   - **前提**：PC 侧必须按 VUI 的 `video_full_range_flag` 转换 ——
     我们 `PREPEND_HEADER_TO_SYNC_FRAMES = 1`，**每个 IDR 都带 VUI**，所以这个前提成立。
4. **PC 侧仍按码流声明转换**（双保险）—— "默认自洽"是**设备行为**，换设备可能不同；
5. **不得相信 `getInputFormat()` 的色彩字段**（实测与实际不符）；
6. **上报链路（两条）**：
   - **能力表**（[`agent/encode-probe.md`](encode-probe.md) §4）：报这三项的**可选值**与**缺省取值**
     ⇒ PC **事先**就知道能选什么、不选会落成什么；
   - **回报**（对「申请屏幕」指令的响应）：报**本次实际生效值**；
   - 而 **PC 侧应以码流 VUI 为准**（最可靠：`PREPEND=1` ⇒ 每个 IDR 都带）。

### 2.5 两个澄清

| 问题 | 回答 |
|---|---|
| **`LIMITED` 会限制色彩吗** | **会** —— Y 被压进 **16–235**（220 级）而非 0–255（256 级），约 **14% 码值浪费**，平滑渐变上易出条带。**`FULL` 对屏幕内容更原生**（源头就是 sRGB 0–255）⇒ **本项目取 `FULL`**。代价是**依赖对端正确读 VUI 的 `video_full_range_flag`**（读错会发灰/发白）；实测往返偏差 **0**，且 `PREPEND=1` 保证每个 IDR 都带 VUI |
| **`SDR_VIDEO` 意味着支持 HDR 吗** | **不** —— 它只是**一个取值**，含义是"这段内容是 SDR"。HDR 对应**另外两个值**：`COLOR_TRANSFER_ST2084`(6，HDR10 的 PQ) 与 `COLOR_TRANSFER_HLG`(7)。**没有叫 `HDR_VIDEO` 的常量**，命名不对称容易误会。**本机主屏确实支持 HDR**（实测 `isHdr=true`，类型 `{HDR10, HDR10+, HLG, Dolby Vision}`，`isWideColorGamut=true`，首选 Display P3），**HEVC 也能编 HDR10**（profile 里有 `Main10HDR10`）—— **但本项目暂不做 HDR，留作后续特性**（见 §3） |

---

## 3. 暂不做、留作后续特性

| # | 项 | 结论 |
|---|---|---|
| 1 | **HEVC Main10（10-bit）** | **暂不做**，留作后续可选特性 |
| 2 | **HDR** | **暂不做**，**作为后续特性适配** —— 本机主屏与 HEVC 编码器都支持（见 [`agent/encode-capability.md`](encode-capability.md) §1.3），但**链路每一步**（手机编码 → 传输 → PC 解码 → 渲染）都要支持，收益对屏幕内容有限 |

---

## 4. 变更记录

- **2026-10-06 初版**：格式组的选择规则（优先默认 → 回退 → **绝不退软编**）、
  本机实测的编码器默认值、色彩元数据的实测与规则。
- **2026-10-06 实测 + 决定**：**色彩元数据**用「已知纯色 → 编码 → ffprobe 读 SPS VUI →
  解码成 YUV 反推矩阵」测出：**`MediaFormat` 报的色彩字段不可信**；**码流 VUI 与内部矩阵自洽**；
  `KEY_COLOR_STANDARD` / `KEY_COLOR_RANGE` 会**同时**改变内部转换与 VUI。
  色彩字段**由 PC 选择、agent 一律显式设置**。
- **2026-10-06 修订**：**色彩 range 缺省由 `LIMITED` 改为 `FULL`** ——
  实测 `COLOR_RANGE_FULL` **端到端成立**（VUI `pc` + 内部 full，8 色往返偏差 **0**）；
  并明确**两条上报链路**（能力表报可选值+缺省、回报报实际生效值；PC 以 VUI 为准）。
- **2026-10-06 收尾**：**10-bit 与 HDR 暂不做、留作后续特性**。
- **2026-10-06 拆分**：本文件从 [`agent/encode.md`](encode.md) 的 §5 **整段提成独立文件**
  （原文件超单文件上限；其中"显式设置总表"移入 [`agent/encode-params.md`](encode-params.md) §6）；
  § 号重排，跨文件引用改指新文件。
