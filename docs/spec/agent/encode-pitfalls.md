# `:agent` · 编码：边界发现与重测指引（F3）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/encode-capability.md`](encode-capability.md)
> （编码器能力**实测快照**）的**配套**：这里放**踩过的坑**与**可复现的测法**。
> 规范见 [`agent/encode.md`](encode.md) 及其子文档；数值见 [`agent/encode-capability.md`](encode-capability.md)。

---

## 1. 边界发现（值得记住的坑）

1. **非法刷新率会"悄悄回退"** —— 请求 `0.5 Hz` 被拒后，回退到的是**主屏模式**（120 Hz），
   **不是 60** ⇒ 非法值表现为"用了别的值"，**必须回读确认**（印证 [`workflow.md`](../workflow.md) §4 全局规则）。
2. **`color-standard = 0`（UNSPECIFIED）不是"留空"** —— 被编码器替换成 `4`。
   ⇒ 想表达"未指定"**不能传 0**，必须显式给一个真值。
3. **`MediaFormat` 报的色彩字段不可信** —— 不设时 `getInputFormat()` 报 BT709、
   `getOutputFormat()` 报 BT601_PAL，而**真值**（码流 VUI + 内部矩阵）是 BT.601；
   显式设置后两者才一致。详见 [`agent/encode-format.md`](encode-format.md) §2。
4. **`prepend-sps-pps-to-idr-frames` 默认 `0`** —— 不显式设 `1` 的话，IDR **不带** SPS/PPS。
   实测（3 秒 / 3 个 IDR）：设 `1` 时 SPS/PPS 各出现 **4** 次且 **3/3 个 IDR 前紧跟 SPS+PPS**；
   设 `0` 时各只出现 **1** 次、**仅 1/3** 个 IDR 带参数集。
   ⇒ 本项目**固定设为 1**（见 [`agent/encode-params.md`](encode-params.md) §4）。
5. **`getAchievableFrameRatesFor` 不可信** —— 报 1440×3200 可达上限 48.96 fps，
   实测却**稳定喂 60.014 fps** 且延迟不增长（见 [`agent/encode-params.md`](encode-params.md) §1.2）。**别拿它当判据**。
6. **`KEY_BIT_RATE` 必填** —— 不设它 `configure()` 抛 `IllegalArgumentException`（**消息为空**），
   与是否设码率模式无关 ⇒ **没有"不设码率、让编码器自己算"这条路**。
7. **码率模式只支持 VBR 与 CBR** —— 实测 `isBitrateModeSupported`：HW 与 SW 编码器
   **都不支持 `CQ`**（`BITRATE_MODE_CQ=0`）；显式设 CQ 会抛 `IllegalArgumentException`。
   常量值：`CQ=0` · **`VBR=1`** · `CBR=2` · `CBR_FD=3`（输出格式回读 `bitrate-mode=1` 即 VBR）。
8. **静止屏的"制造 damage"手段（实测找到两条）** ——
   ① **换输出面**：静止屏 `setSurface(新面)` 后新面**收到 1 帧**（切回旧面也 1 帧，稳定复现）；
   ② **`resize(同尺寸, 改 DPI)`**：2.5 秒内**出 11 帧**（触发重新布局）。
   ⇒ 推翻了原型"未找到主动刷帧手段"的记载。
9. **静止时编码器产不出 IDR** —— 不喂任何帧、只发 `PARAMETER_KEY_REQUEST_SYNC_FRAME`：
   1.5 秒内**新增输出 0 帧**；对照（喂 1 帧 + 请求）⇒ 输出 1 帧且是 **IDR**。
   ⇒ **编码器必须有输入帧才能产 IDR**。
10. **`KEY_REPEAT_PREVIOUS_FRAME_AFTER` 未被采纳** —— 设 `100000 us` 后停止喂帧静置 700ms，
    **新增输出 0 帧**（若生效应有 ~7 帧）。
11. **静止屏上的冻结 / 解冻 —— 完全可行**（零拷贝链路下，屏输出面 = 编码器输入面，
    **复用同一个编码器、不重建**）。**前置条件已验实**（连续两个窗口 0 帧确认屏静止）：

    | 阶段 | 实测 |
    |---|---|
    | 屏确认静止 | 连续两窗口（2s + 2.2s）均 **0 帧** |
    | **冻结** `setSurface(null)` | 2 秒内 **0 帧** ⇒ 真的停输出 |
    | **解冻** `setSurface(同一个输入面)`，**不请求同步帧** | **10 帧、IDR 0** |
    | 解冻后**再观察 2 秒** | **0 帧** ⇒ **一次性爆发，之后重新静止** |
    | 再冻结 / 再解冻（**先请求同步帧**） | 0 帧 / **10 帧、IDR 1** ⇒ 可反复、能拿到 IDR |

    ⇒ **不需要重建编码器**；解冻带来的帧是**一次性的**（换面触发渲染），**不是持续流**。
12. **`KEY_I_FRAME_INTERVAL` 按"编码的帧"计，不是墙钟** —— 解冻后不请求同步帧时，
    1.5 秒内出了 10 帧却**一个 IDR 都没有**（10 帧 ≈ 0.17 秒流时间 < 1 秒间隔）。
    ⇒ **静止屏上"周期性 IDR"永远不会来** ⇒ 需要 IDR 时**必须显式请求**。
13. **`COLOR_RANGE_FULL` 端到端成立**（8 个已知纯色 + 解码成 YUV 反推）：

    | 配置 | 码流 VUI | 实测内部 | 往返偏差 |
    |---|---|---|---|
    | BT709 + **FULL** | `pc` / `bt709` ×3 | BT.709 full | **0** |
    | BT601_NTSC + **FULL** | `pc` / `bt470bg`·`smpte170m` | BT.601 full | **0** |

    ⇒ **range 与 standard 一样会同时改变内部转换与 VUI**，两边一致。

---

## 2. 测法（可复现）

| # | 测什么 | 怎么测 |
|---|---|---|
| 1 | **编码器清单与能力** | 进程内 `MediaCodecList(REGULAR_CODECS)` → 逐 `MediaCodecInfo` 取 `isHardwareAccelerated()` / `getSupportedTypes()` / `getCapabilitiesForType()` 的 `getMaxSupportedInstances()` / `profileLevels` / `getVideoCapabilities()`。**帧率要按尺寸查**：`getSupportedFrameRatesFor(w,h)` / `areSizeAndRateSupported(w,h,fps)` / `getSupportedWidthsFor(h)` |
| 2 | **虚拟屏刷新率** | `VirtualDisplayConfig.Builder.setRequestedRefreshRate(x)` 建屏，回读 `Display.getMode().getRefreshRate()` 与 `getSupportedModes()` |
| 3 | **色彩元数据** | 喂 **8 个已知纯色**（红/绿/蓝/白/黑/灰128/黄/青）→ 编码 → **`ffprobe` 读 SPS VUI** → **解码成 YUV 读 Y 值反推内部矩阵**（BT.601 与 BT.709 对纯色的 Y 差很大，如纯红 Y=81 vs 63，判据不含歧义） |
| 4 | **延迟基线** | EGL 把编码器输入面当 `EGLSurface`，按固定帧率喂帧，量"输出到达时刻 − 该帧提交时刻"。**同时量实际喂帧速率**，否则分不清"编码器跟得上"与"生产端本来就慢" |
| 5 | **并发路数** | 逐路 `createEncoderByType("video/avc")`（**让框架自己选**，才能检出静默落软编）→ configure → `createInputSurface` → start；**再各喂几帧确认真的出帧**（"start 成功 ≠ 能用"） |

**四个踩过的坑（重测时别再踩）**：

- 第 3 项**不要用"解码成 RGB 再比对"** —— 那会把**解码端的矩阵选择**混进来，判据不再干净
  （实测强制不同矩阵结果一样，因为参数根本没生效）。
- 第 4 项：`eglPresentationTimeANDROID` 收的是**纳秒**。传微秒会让输出 PTS 缩小 1000 倍、
  **帧序号全算成 0**，延迟随帧号线性增长（看起来像"编码器跟不上"，其实是量错了）。
- 第 5 项：失败时必须 `release()` 那个半成品编码器，**否则它占着资源会把已成功的那几路一起拖死**
  （实测踩过：7 路全部 start 成功却一帧都不出）。
- 建屏**必须用隐藏构造器 `DisplayManager(Context)`** 塞伪装 Context，`getSystemService()` 给的是
  真系统 Context（包名 `android`）会被拒 —— 见 [`agent/display.md`](display.md) §2.2。

---

## 3. 变更记录

- **2026-10-06 初版**：从 [`agent/encode-capability.md`](encode-capability.md) 拆出（原文 §6、§7）——
  该文件超出 [`docs.md`](../docs.md) §2 的单文件上限，按"内聚子主题整段提成同级文件"拆分。
  内容：13 条边界发现 + 5 项可复现测法 + 4 个重测坑。
