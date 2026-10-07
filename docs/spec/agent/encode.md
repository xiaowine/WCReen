# `:agent` · 编码（F3）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/module.md`](module.md) §2 的子系统 #2，
> 覆盖功能 **F3（采集并零拷贝编码）**。**本文件是入口**：范围与文档地图。
> 显示源（F1）见 [`agent/display.md`](display.md) 与 [`agent/display-lifecycle.md`](display-lifecycle.md)。
> PC 侧解码与容量（**不做容量处理**）见 [`tech-stack/pc.md`](../tech-stack/pc.md) §3。

---

## 1. 范围（要定什么）

虚拟屏的输出面**就是**编码器的输入面（零拷贝 —— 见 [`agent/module.md`](module.md) §3 不变量 3）。
F3 要定六件事，**全部已定**：

| # | 要定的事 | 关联 | 结论在哪 |
|---|---|---|---|
| 1 | 编码器**能力探测**与**能力表**（尺寸/对齐/profile/位深/码率/并发） | 尺寸可行性 = 编码器能力（**编码尺寸必须 = 屏尺寸**，[`agent/display.md`](display.md) §2） | [`agent/encode-probe.md`](encode-probe.md) |
| 2 | codec / 位深的选择，以及**与 PC 硬解能力的对齐** | [`tech-stack/pc.md`](../tech-stack/pc.md) §2 | [`agent/encode-format.md`](encode-format.md) |
| 3 | 码率 / 帧率 / 关键帧间隔 / **低延迟参数** | [`agent/display.md`](display.md) §2（刷新率） | [`agent/encode-params.md`](encode-params.md) |
| 4 | **静止画面无帧**的应对（虚拟屏由 damage 驱动合成） | [`agent/module.md`](module.md) §3 不变量 7 | [`agent/encode-lifecycle.md`](encode-lifecycle.md) §1 |
| 5 | 运行时可变参数与不可变的 | [`agent/display.md`](display.md) §2.1 | [`agent/encode-lifecycle.md`](encode-lifecycle.md) §2 |
| 6 | 编码器的重建/解冻路径与**补关键帧**（硬约束 8） | [`agent/display-lifecycle.md`](display-lifecycle.md) §3 | 同第 4 项（**已合并**） |

> **载体**：本子系统定义的编码参数（码率、低延迟、格式等）都是 **「申请屏幕」指令**的字段 ——
> 与屏参数**同一条指令**、由 **PC 选择**。见 [`agent/display.md`](display.md) §2.0。

---

## 2. 文档地图

| 文件 | 内容 |
|---|---|
| **本文件** | 入口：范围、文档地图 |
| [`agent/encode-probe.md`](encode-probe.md) | **能力探测**：取法（公开 API）、并发协商规则、**能力表结构**、尺寸与帧率的**校验规则**、实机验证结果 |
| [`agent/encode-params.md`](encode-params.md) | **编码参数**：低延迟、码率（表达/优先级/模式/多路）、帧率、关键帧间隔，以及**我们显式设置的参数总表** |
| [`agent/encode-format.md`](encode-format.md) | **格式组**：选择规则（优先默认 → 回退 → 绝不退软编）、**色彩元数据**、暂不做项（10-bit / HDR） |
| [`agent/encode-lifecycle.md`](encode-lifecycle.md) | **生命周期**：**静止态可解码性**（冻结/解冻/IDR）、**参数变更一律重建** |
| [`agent/encode-capability.md`](encode-capability.md) | **本机实测快照**：编码器清单、格式组枚举、范围与步进、并发实测、缺省解析结果（**换设备即作废**） |
| [`agent/encode-pitfalls.md`](encode-pitfalls.md) | **边界发现与重测指引**：踩过的坑 + 可复现测法 |
| [`agent/encode-changelog.md`](encode-changelog.md) | **变更记录**（从本文件 §3 提成独立文件） |

**四条贯穿全 F3 的纪律**（各子文档都会引用）：

1. **不依赖默认值**（[`workflow.md`](../workflow.md) §4 全局规则）：参数**一律显式设置**并**回读确认**；
   协议可缺省，**代码里必须写死或推导出显式值**；
2. **优先硬编；没有硬编时退软编**（用户取向 2026-10-07）：能力表里**如实标记**每个编码器的
   `hardware`（`false` = 软编回退），由 PC 侧决定是否接受 —— PC 默认接受但会**显式提示**
   （软编的尺寸/帧率上限低、设备 CPU 占用高）。**不做静默降级**：降级必须出现在上报里；
3. **探测给候选，试跑给结论**：声明值只是上界，实际以试跑为准；
4. **判据必须可观测**：不接受"没报错"。

---

## 3. 变更记录

**本文件只留指针** —— 变更记录已整段提成 [`agent/encode-changelog.md`](encode-changelog.md)
（本文件再次触到单文件上限，见 [`docs/spec/docs.md`](../docs.md) §2）。
