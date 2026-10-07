# `:agent` 功能清单

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/module.md`](module.md) §2 的**功能级**索引。
> 作用：`:agent` **要做什么**的**单一事实来源** —— 新功能先在这里登记，再讨论，再落细节文档。
> 状态口径：`待讨论` / `讨论中` / `已定` / `已实现` / `后置` / `不做`。
>
> **维护规则**：功能状态**只在本文件维护**；细节文档只写"怎么做"，不重复状态。

---

## 1. 当前范围的功能

| # | 功能 | 一句话目标 | 细节文档 | 依赖 | 状态 |
|---|---|---|---|---|---|
| **F1** | **建独立虚拟屏**（扩展模式） | 新建一块**不影响本机屏**的独立屏，输出面交给编码器 | [`agent/display.md`](display.md) + [`agent/display-flags.md`](display-flags.md) + [`agent/display-lifecycle.md`](display-lifecycle.md) | — | **已定 · 已实现**（S2；`dumpsys display` 查到屏名、回读 flags 校验通过） |
| **F2** | **在虚拟屏上跑 App** | 两条**独立指令**：**枚举可启动应用**（任意时刻、**无屏可用**）+ **打开目标应用**（任意时刻、可反复切换） | [`agent/launch.md`](launch.md) + [`agent/launch-enumerate.md`](launch-enumerate.md) | F1 | **已定 · 已实机验证**（V1–V6 全有结论） |
| **F3** | **采集并零拷贝编码** | 虚拟屏的输出面**就是**编码器输入面，帧不经用户态内存 | [`agent/encode.md`](encode.md)（**入口**）+ probe / params / format / lifecycle / capability / pitfalls | F1 | **已定 · 已实现**（S2；**优先硬编、无硬编退软编**并如实上报 `hardware`；真机 7 个硬编编码器、模拟器 3 个软编；ffmpeg 解出 1920×1080 图） |
| **F4** | **把码流传到 PC** | 协议（握手 / 解码配置 / 控制消息）+ 连接生命周期 + 背压 + **传输可插拔（adb / 网络，须传输无关）** | [`agent/transport.md`](transport.md)（入口）+ `transport-` `topology` / `handshake` / `protocol` / `messages` / `backpressure` | F3 | **已定 · 已实现**（S1 连接层 + S2 **帧路径**：49 帧实测、丢帧到 IDR、背压四动作） |
| **F5** | **接收并注入输入** | 触摸 / 按键 / 滚轮回注到虚拟屏 | [`agent/input.md`](input.md) | F1、F4 | **已定·已实现**（agent S4 + PC 侧；均已实机验收） |
| **F6** | **生命周期与失败处理** | 启动方式（脱离 adb）、`main()` 编排、线程模型、退出与收尾、错误策略 | [`agent/runtime.md`](runtime.md)（入口）+ `runtime-startup` / `runtime-threads` / `runtime-lifecycle` / `runtime-failure` | 横切 | **已定 · 已实现**（S1；两个未验风险：`runtime-startup.md §4`、`runtime.md §2.2`） |
| **F7** | **可观测性** | 日志与心跳 —— 让上面每条功能的验收都有**可观测判据** | [`agent/runtime-observability.md`](runtime-observability.md) | 横切 | **已定 · 已实现**（S1；心跳 **3s** / **10s** 判据；**日志 = 复用 adb logcat**） |

> **F8（参数入口与调试开关）已撤销**（2026-10-07）—— 它要做的三件事都被别处吸收完了：
> **参数集**由 `applyScreen` 承载、**调试开关**决定不加、**CLI 契约**并入
> [`agent/runtime-startup.md`](runtime-startup.md) §1.1。
> **编号 F8 不再复用**（历史引用保持有效）；后置功能仍用 F9–F11。

---

## 2. 后置功能（记录在此，避免遗忘）

| # | 功能 | 为什么后置 | 状态 |
|---|---|---|---|
| **F9** | **镜像模式**（捕获现有屏投到 PC） | 与扩展模式是**两条不同技术路径**（镜像走 `SurfaceControl`，非 `MediaProjection`）；尺寸/DPI 由本机屏固定不可选；**输入注入目标是本机屏**（会真的操作手机），与"不影响本机屏"的初衷语义不同。先把扩展模式做扎实 | **后置** |
| **F10** | **音频** | 未设计；`AudioPlaybackCapture` 受 App 开关限制，另有远端 submix 路线 | **后置** |
| **F11** | **多点触控 / 文本输入** | 属 F5 的能力扩展，待单指链路验证后再加 | **后置** |

---

## 3. 明确不做

- ❌ 用 `MediaProjection` 替代虚拟屏（硬约束）
- ❌ 在 `:agent` 里做 UI
- ❌ 多点触控**以上**的手势识别（捏合、旋转等由 PC 侧合成后按原始触点下发）
- ❌ 修改本机屏的行为（扩展模式的核心承诺）

---

## 4. 依赖与推进顺序

```
F1 建屏 ──► F3 编码（强耦合：编码尺寸 = 虚拟屏尺寸）
   └────► F2 拉起应用（两条独立指令：枚举 / 打开）
F3 ──► F4 传输 ──► F5 输入（复用同一条连接的控制通道）
F6 生命周期 / F7 可观测性 = 横切（骨架可先定，细节随各功能补）
```

**讨论顺序**：按上表编号 F1 → F3 → F2 → F4 → F5；F6/F7 横切（其中 F7 越早越好，
否则各功能的验收都只能停在纸面）。

**实施顺序与讨论顺序不同**：「业务参数走协议 + 启动阶段不建屏 + agent 不自动建屏」
⇒ **建屏只能由 PC 经协议触发** ⇒ **F4 的连接层必须最先**。切片见
[`plans/2026-10-07-agent-slicing.md`](../../plans/2026-10-07-agent-slicing.md)：
S1 = F6 + F7 + F4 连接层（**已交付**）· S2 = F1 + F3 + F4 帧路径（**已交付**）· S3 = F2 ·
S4 = F5（**已交付**）· S5 = 多路与生命周期语义。

> **F2 的依赖**：功能上依赖 F1（要有屏才能"打开"），但**枚举指令本身不依赖屏**（见
> [`agent/launch.md`](launch.md) §2）。

---

## 5. 变更记录

**见 [`agent/features-changelog.md`](features-changelog.md)**（本记录因超出单文件上限已提成独立文件）。
