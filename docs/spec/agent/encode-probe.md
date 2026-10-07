# `:agent` · 编码器能力探测与能力表（F3）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/encode.md`](encode.md)（F3）的**能力探测**部分。
> 编码参数见 [`agent/encode-params.md`](encode-params.md)；格式组见 [`agent/encode-format.md`](encode-format.md)；
> **本机实测数值**见 [`agent/encode-capability.md`](encode-capability.md)；**踩过的坑与重测方法**见 [`agent/encode-pitfalls.md`](encode-pitfalls.md)。

---

## 1. 取法：公开 API，进程内即可

**不需要隐藏 API、不需要建屏**，`app_process` 里直接查：

```java
MediaCodecList list = new MediaCodecList(MediaCodecList.REGULAR_CODECS);
for (MediaCodecInfo info : list.getCodecInfos()) {
    if (!info.isEncoder()) continue;
    for (String type : info.getSupportedTypes()) {
        CodecCapabilities caps = info.getCapabilitiesForType(type);

        info.isHardwareAccelerated();     // 是否硬编 —— **如实记录**：优先硬编，没有硬编才用软编（上报里标记）
        caps.getMaxSupportedInstances();  // 系统声明的并发实例数（§3）
        caps.getVideoCapabilities();      // 尺寸/对齐/帧率/码率范围；音频与图像编码器没有，须判空
        //   getSupportedWidths() / getSupportedHeights()
        //   getWidthAlignment() / getHeightAlignment()
        //   getSupportedFrameRates() / getBitrateRange()
    }
}
```

> `getVideoCapabilities()` 对**非视频**编码器会抛异常 ⇒ 必须 try/catch 或按 mime 前缀过滤。

**⚠️ 能力查询必须用「尺寸相关」的接口，不能只看扁平范围**：

| 用途 | 用哪个 |
|---|---|
| 某尺寸能不能编 | `isSizeSupported(w, h)` |
| **某尺寸 + 某帧率能不能编**（**主判据**） | `areSizeAndRateSupported(w, h, fps)` |
| 某尺寸下的帧率范围 | `getSupportedFrameRatesFor(w, h)` |
| 某尺寸下的可用宽 / 高 | `getSupportedWidthsFor(h)` / `getSupportedHeightsFor(w)` |

`getSupportedFrameRates()` 返回的**扁平范围**（如 `[1, 480]`）**不是真实约束** ——
帧率上限随尺寸变化（本机实测：1920×1080 支持到 480，1440×3200 只到 217.6）。

> **`getAchievableFrameRatesFor(w,h)` 不要当判据** —— 它报的区间与实测**矛盾**
> （本机 1440×3200 报"可达上限 48.96 fps"，但实测**稳定喂 60.014 fps** 且延迟不增长），
> 疑似是"吞吐窗口"而非"上限"。判据用 `areSizeAndRateSupported` + **实跑验证**。

---

## 2. 本机实测值

**完整快照见 [`agent/encode-capability.md`](encode-capability.md)** —— 编码器清单、格式组枚举、
范围与步进、声明并发、缺省解析结果、边界发现与可复现测法。

> ⚠️ 那里的数值是**某台设备的观测值，不是规则** —— **代码里绝不可硬编码**，换设备必须重测。

---

## 3. 并发路数的协商规则（已定）

```
可用路数（上界候选） = min( PC 配置的上限 , 系统声明的 maxSupportedInstances )
```

- **PC 侧设上限**（例：**5**），避免把手机拖死；上限是 PC 的策略，不由 agent 决定；
- agent 在协商时**读取声明能力并回报**，双方取**最小值**；
- 协商消息的字节格式属 **F4**，本节只锁**语义**。

**但这是候选，不是结论**，四条理由：

1. 声明值是"保证值"而非"实测值"，与分辨率强相关；
2. `maxSupportedInstances` 是**全系统共享**的预算 —— 手机正在录屏 / 通话 / 录像时留给我们的更少；
3. **还有一个"尺寸相关的资源上限"** —— 实测本机在 1440×3200@60 下**只能开 7 路**
   （声明是 16），而 ≤3.3 Mpixel 时能开到 14–15 ⇒ **两个上限取小者**，
   且该限制**与帧率无关**（60fps 与 30fps 同为 7）⇒ 卡的是缓冲区 / 会话资源。
   详见 [`agent/encode-capability.md`](encode-capability.md) §4.1；
4. **"start 成功" ≠ "能用"** —— 实测踩过：资源被半成品占用时，7 路**全部 start 成功却一帧都不出**
   ⇒ **每一路都必须验证真的出帧**，不能只看 `configure` / `start` 没报错。

> **失败形态（实测）**：抛 `MediaCodec$CodecException`（**消息为空**），
> **没有静默落软编**（全部尝试拿到的都是 HW），**也不影响已成功的那几路**。
> 纪律**调整为**：「**验证编码器属性与上报一致 + 验证真出帧**」—— 现在允许软编回退，
> 所以不再"发现不是硬编就拒绝"，而是**比对能力表里的 `hardware` 与实际的
> `isHardwareAccelerated()`，不一致即记警告**（跨设备安全网保留）；出帧验证照旧。

⇒ 最终路数**仍以试跑为准**：**探测给候选，试跑给结论**。

---

## 4. 能力表（agent → PC 上报的结构）

**要解决的问题**：格式参数的缺省是**按设备动态推导**的（见 [`agent/encode-format.md`](encode-format.md) §1），
PC 若不填，**事先不知道** agent 会选什么 —— 等回报才知道就晚了。

**解法**：agent 把**结构化能力表**上报给 PC；PC 据此决策，**缺省时也能预知结果**。

**能力表必须表达三类形式 —— 不能只给"最高支持什么"**：

| 形式 | 用在哪些参数 | 例（本机实测） |
|---|---|---|
| **枚举**（有序，含"第一个"的语义） | codec、profile、位深、色彩 | `codec ∈ {H.264, HEVC}`；`profile(H.264) ∈ {Baseline, Main, High}`；`位深 ∈ {8, 10}` |
| **范围 + 步进** | 尺寸、帧率（刷新率）、码率 | `尺寸 [128, 8192] 步进 2`；`帧率 [1, 480]`；`码率 [1, 220000000]` |
| **整数** | 并发路数（**声明值**，见 §3） | `16` |

**四条硬要求**：

1. **表的每一行都要带「缺省取值」** —— PC 不填任何字段时，也能**事先知道** agent 会选什么；
   缺省的推导规则见 [`agent/encode-format.md`](encode-format.md) §1（优先默认 → 不支持则退到第一个支持的）。
2. **逐编码器列出**，不能只报一个 —— 同一 mime 下可能有多个编码器且**能力不同**
   （见 [`agent/encode-capability.md`](encode-capability.md) §1.1：`…hevc.encoder.cq` 最大只到 512×512）。
3. **枚举必须有序** —— 因为回退规则是"取**第一个**支持的"，顺序本身就是信息。
4. **不得假定任何"常见值"** —— 尺寸对齐不一定是 16（本机是 **2×2**）；
   codec 不能按名字或 mime 筛（同 mime 下能力可能差极大，本机 `…hevc.encoder.cq` 最大只到 512×512）
   ⇒ **一律读能力再判断**。

> 上报的**载体与时机**（握手时给 / 单独的查询指令）归 **F4**；本节只锁**结构与内容**。

---

## 5. 尺寸与帧率的校验规则（已定）

**尺寸与帧率是耦合的**（帧率上限随尺寸变化，见 §1）⇒ **必须成对校验**，不能各自独立判。

请求进入后的判定顺序（**全部通过才应用**）：

| # | 判据 | 用什么 |
|---|---|---|
| 1 | **对齐** —— `w % alignW == 0 && h % alignH == 0` | `getWidthAlignment()` / `getHeightAlignment()`（**本机是 2，不是 16**，见 §4 硬要求 4） |
| 2 | **尺寸本身可编** | `isSizeSupported(w, h)` |
| 3 | **该尺寸下的宽 / 高范围** | `getSupportedWidthsFor(h)` / `getSupportedHeightsFor(w)`（**不能**用扁平的 `getSupportedWidths()` 代替） |
| 4 | **尺寸 + 帧率成对可编** | `areSizeAndRateSupported(w, h, fps)`，其中 `fps` = **刷新率** |
| 5 | 编码器帧率范围 | `getSupportedFrameRatesFor(w, h)`（仅用于**给出拒绝原因**，不作主判据） |

**三条纪律**：

1. **PC 显式给的值不满足即拒绝**（带**具体哪一项不满足**），**不静默修正、不静默降级** ——
   尺寸直接决定 PC 侧窗口大小，悄悄改会让 PC 的布局预期落空。
   > 与"格式组回退"（[`agent/encode-format.md`](encode-format.md) §1）的区别：那里是**枚举**且 PC 未指定时才有回退；
   > 这里是**范围**，PC 指定了就必须满足。
2. **缺省值也要过同一套校验** —— 缺省尺寸 = 主屏分辨率、缺省刷新率 = 60（见
   [`agent/display.md`](display.md) §2）。**不过就报错**（不能悄悄换尺寸）。
3. **刷新率本身要回读确认** —— 非法值（如 `0.5`）会被**悄悄回退到主屏模式**，
   不是回退到 60（见 [`agent/encode-pitfalls.md`](encode-pitfalls.md) §1 第 1 条）。

> 校验通过后：**编码尺寸 = 屏尺寸**（F1 硬约束），无需再做一次换算。

---

## 6. 实机验证结果

**§3 的 4 项并发验证已跑完**（完整数据见 [`agent/encode-capability.md`](encode-capability.md) §4.1）：

| # | 原未知 | 结论 |
|---|---|---|
| 1 | 第几路开始失败 | **与尺寸强相关**：≤3.3 Mpixel 可到 **14–15** 路；**1440×3200@60 只有 7 路** |
| 2 | 失败形态 | **抛 `CodecException`（消息为空）**；**无静默软编**；**不影响已成功的那几路** |
| 3 | 不同分辨率 / 帧率的差异 | 尺寸影响极大；**帧率无影响**（60fps 与 30fps 同为 7）⇒ 卡**缓冲区 / 会话资源**，非吞吐 |
| 4 | 系统已有其他编码任务时 | **部分测**：测时有 **1 路被别的进程占用**（故小尺寸下是 15 而非 16）；**0 占用的干净基线未测** |

**两项数值决定不测**（**不影响设计**，理由如下）：

| # | 项 | 为什么不影响 |
|---|---|---|
| 1 | **0 占用的干净并发基线** | 现有数字是在**另有 1 路被占用**时测的 ⇒ 属**保守侧**（真实可用路数只会更多）⇒ 拿它做保守估计是安全的。且 **PC 侧不做任何容量处理**（[`tech-stack/pc.md`](../tech-stack/pc.md) §3）⇒ 本来就不需要一个精确的静态数字 |
| 2 | **更细的尺寸拐点**（3.3 M 与 4.6 M 之间） | 设计上**不需要知道精确拐点** —— 能否开某一路由**实际试跑**决定；**PC 侧不做容量处理** ⇒ 没有"上限该设多少"这回事 |

---

## 7. 变更记录

- **2026-10-06 初版**：编码器能力探测定稿 —— 取法（公开 API `MediaCodecList`）、
  本机实测参考值、**并发路数的协商规则 `min(PC 上限, 系统声明值)`**，
  以及"声明值只是上界候选、必须逐路验证是硬编、最终以试跑为准"三条纪律。
- **2026-10-06 补测**：增补「**能力查询必须用尺寸相关接口**」——
  帧率上限**随尺寸变化**（1440×3200 只到 217.6、1920×1080 到 480），
  扁平 `getSupportedFrameRates()` 不是真实约束；且 **`getAchievableFrameRatesFor` 与实测矛盾**
  （报 48.96 上限，实测稳定喂 60.014 fps）⇒ **判据用 `areSizeAndRateSupported` + 实跑**。
- **2026-10-06 新增**：**§4 能力表**（枚举 / 范围+步进 / 整数三类形式 + 四条硬要求）、
  **§5 尺寸与帧率的校验规则**、**§6 实机验证结果**（并发 4 项跑完，两项数值决定不测）。
- **2026-10-06 拆分**：本文件从 [`agent/encode.md`](encode.md) 的 §2 **整段提成独立文件**
  （原文件超单文件上限）；§ 号重排，跨文件引用改指新文件。
- **2026-10-07**：§6 的「两项数值决定不测」理由改写 —— 原文以「容量本来就探测给候选、试跑给结论」
  与「PC 侧上限该设多少」为依据，现改为「**PC 侧不做任何容量处理**」⇒ 本来就不需要精确的静态数字。
