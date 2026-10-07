# `:agent` · 编码器能力快照（本机实测）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/encode.md`](encode.md)（F3）的**实测快照**。
> **规范**（怎么取、能力表要有哪些形式、怎么用）在 [`agent/encode-probe.md`](encode-probe.md) 与
> [`agent/encode-format.md`](encode-format.md)；**踩过的坑与重测方法**在 [`agent/encode-pitfalls.md`](encode-pitfalls.md)。
>
> ⚠️ **本文件是某台具体设备的观测值，不是规则**：
>
> - **代码里绝不可硬编码**这里的任何数值；
> - **换设备 / 系统升级后必须重测**（本仓库纪律：原型与历史版本的实测值一律不适用，
>   见 [`workflow.md`](../workflow.md) §4）。

**设备**：Redmi 22127RK46C（`socrates`）· Android 16 / SDK 36 · 测于 2026-10-06 ·
测试身份 `app_process` / uid 2000。

---

## 1. 编码器清单

### 1.1 硬件编码器（**候选**）

| 编码器 | mime | profile | 尺寸范围 | 对齐 | 帧率 | 码率范围 | 声明并发 |
|---|---|---|---|---|---|---|---|
| **`c2.qti.avc.encoder`** | video/avc | Baseline / Main / **High** | 128–8192 | 2×2 | 1–480 | 1 – 220 M | **16** |
| `OMX.qcom.video.encoder.avc` | video/avc | 同上（**能力完全相同的别名**） | 128–8192 | 2×2 | 1–480 | 1 – 220 M | 16 |
| **`c2.qti.hevc.encoder`** | video/hevc | Main / MainStill / **Main10** / Main10HDR10 / Main10HDR10Plus | 96–8192 | 2×2 | 1–480 | 1 – 160 M | **16** |
| `OMX.qcom.video.encoder.hevc` | video/hevc | 同上（别名） | 96–8192 | 2×2 | 1–480 | 1 – 160 M | 16 |
| `c2.qti.hevc.encoder.cq` | video/hevc | 同上 | **128–512** | 2×2 | 1–480 | 1 – 160 M | 16 |
| `c2.qti.hevc.encoder.hdr` | video/hevc | 同上 | 128–4096 × 128–**2176** | 2×2 | **1–60** | 1 – 160 M | 16 |
| `c2.dolby.encoder.hevc` | video/dolby-vision | 256（Dolby） | 96–8192 | 2×2 | 1–480 | 1 – 160 M | 16 |

**三条要注意的**：

1. **同一 mime 下能力差别很大** —— `…cq` 只到 512×512、`…hdr` 只到 60fps 且高度上限 2176。
   ⇒ 这正是 [`agent/encode-probe.md`](encode-probe.md) §4 要求"**逐编码器列出**"的原因。
2. **`profileLevels` 里的 level 是厂商编码值**（如 `131072`、`33554432`），**不是标准 AVC/HEVC level**
   ⇒ 不要拿它跟规范里的 level 数字比。
3. `OMX.qcom.*` 与 `c2.qti.*` 是**同一实现的别名**（能力逐项相同）⇒ 选编码器时任取其一。

### 1.2 软件编码器（**不用**，列出以说明为何不可用）

| 编码器 | mime | 尺寸范围 | 码率上限 |
|---|---|---|---|
| `c2.android.avc.encoder` / `OMX.google.h264.encoder` | video/avc | 16–2048 | 12 M |
| `c2.android.hevc.encoder` | video/hevc | **2–512** | 10 M |
| `c2.android.av1.encoder` | video/av01 | 2–1920 | 20 M |
| `c2.android.vp8` / `c2.android.vp9` | vp8 / vp9 | 2–2048 | 30–40 M |
| `c2.android.apv` | video/apv | 16–1920 | 240 M |
| `c2.android.h263` / `c2.android.mpeg4` | 3gpp / mp4v-es | ≤176 | ≤128 k |

⇒ **尺寸上限 512–2048，全都撑不住虚拟屏尺寸**（如 1440×3200）⇒ 与
[`agent/encode-format.md`](encode-format.md) §1「**绝不退软编**」一致。

### 1.3 主屏（displayId 0）的显示特性

给"色彩字段缺省怎么推导"与"将来要不要做 HDR"作依据：

| 项 | 实测 |
|---|---|
| 尺寸 / 刷新率 | 1440×3200 @ 120 Hz |
| **colorMode** | **`0` = `COLOR_MODE_DEFAULT`**；支持 `[0, 7, 9]` = `{DEFAULT, sRGB, Display P3}` |
| **广色域** | `isWideColorGamut = true`，首选广色域空间 = **Display P3** |
| **HDR** | `isHdr = true`；类型 `[1, 2, 3, 4]` = `{HDR10, HDR10+, HLG, Dolby Vision}`；最大亮度 **420 nits** |
| **当前 HDR/SDR 比** | **`1.0`** ⇒ **当前是 SDR 显示模式** |

> **`COLOR_MODE_*` 真实取值**（容易记错，记下来）：
> `DEFAULT=0` · `BT601_625=1` · `BT601_625_UNADJUSTED=2` · `BT601_525=3` · `BT601_525_UNADJUSTED=4` ·
> `BT709=5` · `DCI_P3=6` · `SRGB=7` · `ADOBE_RGB=8` · `DISPLAY_P3=9` · `INVALID=-1`
>
> `getColorMode()` / `getSupportedColorModes()` **不是公开 API**（在 `DisplayInfo` 上）⇒ 需反射。

**⇒ 结论**：主屏**支持 HDR 与广色域**，但**虚拟屏是 SDR 的**（`VirtualDisplayConfig` 无色彩模式设置）
⇒ 色彩字段缺省跟**虚拟屏的实际情况**走（SDR），不跟"主屏能不能 HDR"走。

---

## 2. 格式组枚举（**有序**，供"退到第一个支持的"用）

| 参数 | 可选值（按优先级） |
|---|---|
| **codec** | **`H.264`** → `HEVC`（只有这两个有硬件编码；Dolby Vision 非通用；AV1 / VP9 / VP8 只有软编） |
| **profile** | H.264：`Baseline` → `Main` → **`High`**；HEVC：`Main` → `MainStill` → `Main10` → `Main10HDR10` → `Main10HDR10Plus` |
| **位深** | H.264：`8`；HEVC：`8` → `10` |
| **色彩 standard** | `1`(BT709) · `2`(BT601_PAL) · `4`(BT601_NTSC) · `6`(BT2020) —— **均被采纳且回读一致** |
| **色彩 range** | `1`(FULL) · `2`(LIMITED) —— 均被采纳；**`FULL` 已实测端到端成立**（VUI 写 `pc`、内部转换同步，8 色往返偏差 **0**） |
| **色彩 transfer** | `1`(LINEAR) · `2`(SMPTE170M) · `3`(SDR_VIDEO) · `6`(ST2084) · `7`(HLG) —— 均被采纳 |

---

## 3. 范围 + 步进

| 参数 | 范围 | 备注 |
|---|---|---|
| **编码尺寸** | H.264 `[128, 8192]`；HEVC `[96, 8192]` | **步进 2**（不是常见的 16） |
| **虚拟屏刷新率** | 实测 **1 – 1000 Hz 全部精确采纳**（含 `59.94` / `29.97` 非整数） | **不是"可选模式列表"，是任意值**；每次建屏各生成一个独立 `Display.Mode` |
| **编码器帧率** | **扁平范围 `[1, 480]`** —— **但这不是真实约束**，见下 | 与刷新率是两回事（见 [`agent/encode-pitfalls.md`](encode-pitfalls.md) §1 第 1 条） |
| **码率** | H.264 `[1, 220 000 000]`；HEVC `[1, 160 000 000]` | 单位 bps |

### 3.1 帧率上限**随尺寸变化**（扁平范围不够用）

| 尺寸 | `getSupportedFrameRatesFor` | `areSizeAndRateSupported(...,60)` | 性能点 |
|---|---|---|---|
| **1440×3200** | `[1.0, 217.6]` | **true** | 7680×4320@30 · 3840×2160@120 · 1920×1088@240 |
| 1920×1080 | `[1.0, 480.0]` | true | 同上 |

⇒ 能力表里的帧率**必须按尺寸查**（`getSupportedFrameRatesFor(w,h)`），
扁平 `[1, 480]` 只能当"上界的上界"。

### 3.2 ⚠️ `getAchievableFrameRatesFor` 与实测矛盾

| 尺寸 | `getAchievableFrameRatesFor` | **实测** |
|---|---|---|
| **1440×3200** | `[22.2, 48.96]` ← 说上限 48.96 | **稳定喂 60.014 fps**（180 帧 / 2999 ms），**端到端延迟不随时间增长**（中位 21.5 ms、p90 25.3 ms） |
| 1920×1080 | `[49.0, 108.0]` | — |

⇒ 它报的**不是"帧率上限"**（疑似"吞吐窗口"：两个尺寸都落在同一像素吞吐区间）。
**不得当判据** —— 判据用 `areSizeAndRateSupported` + **实跑验证**。

---

## 4. 整数

| 项 | 值 |
|---|---|
| **声明并发实例** | **16**（全部硬件编码器）／ 32（软件） |

> 声明值只是**上界候选**，且是**全系统共享**的预算 —— 见 [`agent/encode-probe.md`](encode-probe.md) §3。

### 4.1 并发实测（**声明值 ≠ 实际可用**）

测法：逐路 `createEncoderByType("video/avc")` → configure → `createInputSurface` → start，
**再各喂 10 帧确认真的出帧**。

| 尺寸 | 像素 | **成功路数** | 第几路开始失败 |
|---|---|---|---|
| 1280×720@60 | 0.9 M | **15** | 16 |
| 960×1600@60 | 1.5 M | **15** | 16 |
| 1920×1080@60 | 2.1 M | **15** | 16 |
| 1080×2400@60 | 2.6 M | **14** | 15 |
| 1280×2560@60 | 3.3 M | **14** | 15 |
| **1440×3200@60** | **4.6 M** | **7** | **8** |
| 1440×3200@**30** | 4.6 M | **7** | 8 |

**四条结论**：

1. **有两个上限，取小者** —— **实例数上限**（声明 **16**）与**尺寸相关的资源上限**
   （尺寸大了先撞它：1440×3200 只有 7）。
2. **与帧率无关** —— 1440×3200 在 **60fps 与 30fps 下都是 7** ⇒ 卡的是**缓冲区 / 会话资源**，不是吞吐。
3. **失败形态明确** —— 抛 `MediaCodec$CodecException`（**消息为空**）；**没有静默落软编**
   （所有尝试拿到的都是 HW）；**也不影响已成功的那几路**。
4. **"start 成功" ≠ "能用"** —— 实测踩过：资源被半成品占用时，7 路**全部 start 成功却一帧都不出**
   ⇒ **每一路都必须验证真的出帧**，不能只看 start 没报错。

> ⚠️ **测量条件**：以上数字是在**另有 1 路编码器被别的进程占用**时测的（原型进程仍持有一块虚拟屏）。
> 干净基线（0 占用）**决定不测** —— 现有数字属保守侧，不影响设计
> （见 [`agent/encode-probe.md`](encode-probe.md) §6）。

---

## 5. 缺省解析结果

按 [`agent/encode-format.md`](encode-format.md) §1 的规则（优先默认 → 不支持则退到第一个支持的）
在本机会解析成：

| 参数 | 解析结果 |
|---|---|
| codec | **`c2.qti.avc.encoder`**（H.264，硬编） |
| 位深 | **8** |
| profile | **`High`** |
| 色彩 | `BT709 + FULL + SDR_VIDEO`（依据：虚拟屏是 SDR + 内容是 sRGB；FULL 不浪费码值，实测往返偏差 0 —— 见 [`agent/encode-format.md`](encode-format.md) §2） |

> 这里记的是"**在本机会落成什么**"（验证证据）；**规则本身**在
> [`agent/encode-format.md`](encode-format.md) §1。
> 且按 [`agent/encode-probe.md`](encode-probe.md) §4，这些缺省值必须**随能力表一起上报给 PC**。

---

## 6. 变更记录

- **2026-10-06 初版**：从 [`agent/encode.md`](encode.md) §2.2 拆出并补全 ——
  编码器清单（含同 mime 下的能力差异）、格式组枚举、范围与步进、声明并发、
  缺省解析结果、边界发现、可复现测法。
- **2026-10-06 补测**：新增 **§3.1 帧率上限随尺寸变化** 与 **§3.2 `getAchievableFrameRatesFor` 与实测矛盾**；
  **§4.1 并发实测**（声明 **16**，但 1440×3200@60 实测只有 **7** 路，且与帧率无关）。
- **2026-10-06 补测**：新增 **§1.3 主屏显示特性**（colorMode / 广色域 / HDR，含 `COLOR_MODE_*` 真实取值）。
- **2026-10-06 拆分**：**§6 边界发现**与**§7 测法**整段提成
  [`agent/encode-pitfalls.md`](encode-pitfalls.md)（本文件超单文件上限）；§ 号不变。
