# WCReen 协作规范（索引）

> 本文件对**在本仓库内工作的所有 AI 编码助手**生效，会话开始即自动加载。
> 它是**索引**，不是全量规范：具体规范按主题与模块拆分为独立文档，见下表。
> 优先级：**用户当次的明确指令 > 本文件 > 各规范文档 > 仓库其他文档的惯例**。
> 与用户的沟通、以及所有产出（含代码注释）一律使用**中文**。

## 项目一句话

在 Android 手机上创建一块**不影响本机屏幕**的独立虚拟屏，投屏到电脑并回传输入。
本仓库是**重构起点**（Gradle / Android 模板）：只有构建骨架与规范文档，
**不含**任何原型源码，也**不含**原型文档。

## 与原型项目的关系

原型在仓库上层 `../WCReen-copy/`，**只作背景**。唯一入口：[`docs/prototype.md`](docs/prototype.md)。

两条硬规矩（细则见该文档）：

- **单点引用**：其他文档需要提及原型时，只链接 `docs/prototype.md`，不直接链接原型内部文件；
- **不继承结论**：本仓库是重构，原型的一切结论、数值与测试验证**一律不适用**，必须在本仓库重新验证。

本仓库必须**自持**：设计依据落在本仓库文档或代码注释，结论必须在本仓库重新表述。

## 规范索引

| 文档 | 覆盖内容 | 何时必读 |
|---|---|---|
| [`docs/spec/workflow.md`](docs/spec/workflow.md) | 讨论 → 方案 → 实施 工作流；方案文档格式、存放与保留约定；实施闸门；自检清单 | **任何代码改动之前** |
| [`docs/spec/git.md`](docs/spec/git.md) | git 提交 / 历史改写 / 推送规则；授权边界 | 涉及版本控制操作时 |
| [`docs/spec/comments.md`](docs/spec/comments.md) | 中文注释规范（类 / 函数 / 结构体及其成员、函数体注释） | 编写或修改任何代码时 |
| [`docs/spec/agent/code.md`](docs/spec/agent/code.md) + [`hiddenapi`](docs/spec/agent/code-hiddenapi.md) | `:agent` **代码形态**：包结构、命名、失败形态、并发原语、DTO、装配、可测性分层；**隐藏 API 访问形态**（尽可能不用反射、运行期 6 处清单、四条纪律） | **写 `agent/` 下代码之前**（与 module.md 并列） |
| [`docs/spec/agent/module.md`](docs/spec/agent/module.md) | `:agent` 模块总览：子系统地图、硬约束、依赖政策、构建部署、契约索引、禁忌 | **改动 `agent/` 任何内容之前** |
| [`docs/spec/agent/display.md`](docs/spec/agent/display.md) | `:agent` 显示源（F1）：范围、参数模型与变更、Context 引导、验收 | 改显示源之前 |
|[`docs/spec/agent/display-flags.md`](docs/spec/agent/display-flags.md)|`:agent` 虚拟屏 flags（F1）：分档清单、对照实验、常量真实数值|改 flags 组合之前|
|[`docs/spec/agent/display-letterbox.md`](docs/spec/agent/display-letterbox.md)|`:agent` 虚拟屏上的**裁切（letterbox）机制**与"铺满"可选做法**留档**（F1）：三条触发条件、两支比例、双端实测矩阵|**排查 App 被裁之前**|
| [`docs/spec/agent/display-changelog.md`](docs/spec/agent/display-changelog.md) | `:agent` 显示源(F1)**变更记录** | 追溯 F1 决策演变时 |
| [`docs/spec/agent/display-lifecycle.md`](docs/spec/agent/display-lifecycle.md) | `:agent` 显示源生命周期:建立、断开与重连、冻结 A/B | 改生命周期 / 重连之前 |
| [`docs/spec/agent/encode.md`](docs/spec/agent/encode.md) | `:agent` 编码（F3）**入口**：范围、文档地图；变更记录见 [`encode-changelog.md`](docs/spec/agent/encode-changelog.md) | 改编码 / 探测能力之前 |
| [`docs/spec/agent/encode-probe.md`](docs/spec/agent/encode-probe.md) | F3 · 能力探测：取法、并发协商、**能力表结构**、尺寸帧率校验 | 探测编码器能力 / 校验参数时 |
| [`docs/spec/agent/encode-params.md`](docs/spec/agent/encode-params.md) | F3 · 编码参数：低延迟、码率、帧率、关键帧，**显式设置总表** | 改编码参数之前 |
| [`docs/spec/agent/encode-format.md`](docs/spec/agent/encode-format.md) | F3 · 格式组：选择规则、**色彩元数据**、暂不做项 | 改 codec / 位深 / 色彩之前 |
| [`docs/spec/agent/encode-lifecycle.md`](docs/spec/agent/encode-lifecycle.md) | F3 · 生命周期：**静止态可解码性**、参数变更一律重建 | 改重建 / 解冻路径之前 |
| [`docs/spec/agent/encode-capability.md`](docs/spec/agent/encode-capability.md) | F3 · 编码器能力**实测快照**（本机，**换设备即作废**） | 换设备重测 / 核对本机能力时 |
| [`docs/spec/agent/encode-pitfalls.md`](docs/spec/agent/encode-pitfalls.md) | F3 · **边界发现与重测指引**：踩过的坑 + 可复现测法 | 重测编码器 / 排查怪现象时 |
| [`docs/spec/agent/launch.md`](docs/spec/agent/launch.md) | `:agent` 拉起应用(F2):两条指令的范围与共同约束、打开指令、失败分类 | 改枚举 / 拉起 App 之前 |
| [`docs/spec/agent/launch-verified.md`](docs/spec/agent/launch-verified.md) | `:agent` 拉起应用(F2)**实机验证结果**(V1-V6 + 附带发现) | 核对 F2 实测结论时 |
| [`docs/spec/agent/module-changelog.md`](docs/spec/agent/module-changelog.md) | `:agent` 模块规范**变更记录** | 追溯模块级决策演变时 |
| [`docs/spec/agent/launch-enumerate.md`](docs/spec/agent/launch-enumerate.md) | `:agent` 枚举可启动应用（F2 枚举指令）：参数、实现路径、字段、缓存与名单口径 | 改 App 枚举之前 |
| [`docs/spec/agent/features.md`](docs/spec/agent/features.md) | `:agent` **功能清单**：范围、状态、依赖、后置与不做 | 想知道"要做什么 / 做到哪了"时 |
| [`docs/spec/agent/features-changelog.md`](docs/spec/agent/features-changelog.md) | `:agent` 功能清单**变更记录**（2026-10-07 起；更早见归档） | 追溯功能范围与状态演变时 |
| [`docs/spec/agent/features-changelog-archive.md`](docs/spec/agent/features-changelog-archive.md) | `:agent` 功能清单**变更记录归档**（2026-10-06 及更早，F1–F3 时代） | 追溯 F1–F3 决策演变时 |
| [`docs/spec/agent/runtime.md`](docs/spec/agent/runtime.md) | `:agent` 运行时（**F6** 生命周期与失败处理 / **F7** 可观测性）：启动与通信解耦（传输无关）、脱离启动、`main()` 编排、日志与心跳、退出与收尾 | 改启动 / 退出 / 日志 / 线程模型之前 |
| [`docs/spec/agent/runtime-threads.md`](docs/spec/agent/runtime-threads.md) | `:agent` **线程模型**（F6）：线程表与 owner、消息路由、传输接缝、零拷贝与背压、收尾与 join 顺序 | 改线程划分 / 队列 / 收尾顺序之前 |
| [`docs/spec/agent/runtime-lifecycle.md`](docs/spec/agent/runtime-lifecycle.md) | `:agent` **生命周期状态与协调**（F6）：显示链路 3 态、合法组合与不变量、协调者职责、幂等与 fatal 规则 | 改状态机 / 拆建屏策略 / 协调职责之前 |
| [`docs/spec/agent/runtime-startup.md`](docs/spec/agent/runtime-startup.md) | `:agent` **启动编排**（F6）：**六步**顺序、线程启动次序、不做残留清理、启动失败与退出码 | 改 `main()` 编排顺序之前 |
| [`docs/spec/agent/runtime-failure.md`](docs/spec/agent/runtime-failure.md) | `:agent` **失败与退出**（F6）：四级错误策略、致命来源、子系统级判据、对外表现、退出码与终止信号 | 改错误分类 / 退出码 / 信号处理之前 |
| [`docs/spec/agent/runtime-observability.md`](docs/spec/agent/runtime-observability.md) | `:agent` **可观测性**（F7）：心跳（周期 / 独立消息 / 超时判据）、日志通道与待定项 | 改心跳 / 日志之前 |
| [`docs/spec/agent/runtime-changelog.md`](docs/spec/agent/runtime-changelog.md) | `:agent` 运行时**变更记录** | 追溯运行时决策演变时 |
| [`docs/spec/agent/transport.md`](docs/spec/agent/transport.md) | `:agent` **传输与协议**（F4）**入口**：范围、文档地图、推进顺序 | 开始 F4 任何子议题之前 |
| [`docs/spec/agent/transport-topology.md`](docs/spec/agent/transport-topology.md) | `:agent` **拓扑与传输**（F4，已定）：单进程多路、`streamId`、线程归属、生命周期与计数语义、传输层 | 改多路结构 / 连接生命周期 / 监听之前 |
| [`docs/spec/agent/transport-handshake.md`](docs/spec/agent/transport-handshake.md) | `:agent` **握手与能力协商**（F4，已定）：版本号、能力表、协商职责、`streamId` 位置、解码配置、握手超时 | 改握手 / 能力协商 / 版本策略之前 |
| [`docs/spec/agent/transport-protocol.md`](docs/spec/agent/transport-protocol.md) | `:agent` **协议格式**（F4，已定）：统一框架、帧负载、控制消息 CRC + JSON、类型表、不分片、字节序 | 改线协议字节格式之前 |
| [`docs/spec/agent/transport-backpressure.md`](docs/spec/agent/transport-backpressure.md) | `:agent` **背压与丢帧**（F4，已定）：两个机制分开、丢到 IDR、四个动作、主动请求 IDR、容量与 `T_drop` | 改丢帧策略 / 队列容量之前 |
| [`docs/spec/agent/transport-messages.md`](docs/spec/agent/transport-messages.md) | `:agent` **消息清单**（F4，已定）：类型表、指令 `cmd`、`applyScreen` 参数、回报与主动上报、错误对象（**变更记录已提成 `transport-changelog.md`**） | 改任一消息的字段之前 |
| [`docs/spec/agent/transport-changelog.md`](docs/spec/agent/transport-changelog.md) | `:agent` **消息清单变更记录**（F4） | 追溯消息清单演变时 |
| [`docs/spec/agent/transport-shapes.md`](docs/spec/agent/transport-shapes.md) | `:agent` **报文形状**（F4，已定）：能力表 / `applyScreen` / 指令回报 / `input` / 错误对象的**确切 JSON** + 12 条 `error.code` 登记表 | 写对端实现 / 改报文 JSON 之前 |
| [`docs/spec/agent/input.md`](docs/spec/agent/input.md) | `:agent` **输入注入**（F5，**已定·已实现**）：四项实测前提与取法、触点快照表示、屏像素坐标、`ASYNC`、队列合并与硬上界、**成功静默** | 改输入注入之前 |
| [`docs/spec/app/module.md`](docs/spec/app/module.md) | `:app` 模块约束：定位、版本下限、未定分叉 | 改动 `app/` 下任何内容之前 |
| [`docs/spec/tech-stack/overview.md`](docs/spec/tech-stack/overview.md) | 技术栈总览：三侧语言/框架、构建链、依赖与测试政策、未决项 | 涉及选型、引入新库或新模块时 |
| [`docs/spec/tech-stack/pc.md`](docs/spec/tech-stack/pc.md) | PC 端：跨平台后端、帧通路与硬解强校验、容量（**不做容量处理**）、输入上报（**F5 PC 侧**）；FFmpeg 接入已提成 [`pc-ffmpeg.md`](docs/spec/tech-stack/pc-ffmpeg.md) | 改 PC 端解码/渲染/输入之前 |
| [`docs/spec/tech-stack/pc-frame-path.md`](docs/spec/tech-stack/pc-frame-path.md) | PC 端**多路合流 / 硬解 / 帧通路**（从 `pc.md` §2 提成独立文件）：三环节分工、T0–T2 成本阶梯、静默软解实测与硬解强校验、延迟量化 | 改帧通路 / 硬解校验之前 |
| [`docs/spec/tech-stack/pc-ffmpeg.md`](docs/spec/tech-stack/pc-ffmpeg.md) | PC 端 **FFmpeg 接入**：动态链接 + 随包分发、被否路径 | 动 FFmpeg 依赖 / 分发方式之前 |
| [`docs/spec/tech-stack/pc-ui.md`](docs/spec/tech-stack/pc-ui.md) | PC 端：UI 层选型与被否方案 | 改 PC 端 UI 之前 |
| [`docs/spec/tech-stack/changelog.md`](docs/spec/tech-stack/changelog.md) | 技术栈**变更记录**（2026-10-07 起；更早见归档） | 追溯技术选型演变时 |
| [`docs/spec/tech-stack/changelog-archive.md`](docs/spec/tech-stack/changelog-archive.md) | 技术栈**变更记录归档**（2026-10-06 追加系列，初版–追加 11） | 追溯早期技术选型演变时 |
| [`docs/spec/docs.md`](docs/spec/docs.md) | 文档规范：分模块、单文件上限（200 行 / 12 KB）、拆分与登记 | **新增 / 修改文档时** |
| [`docs/code-map.md`](docs/code-map.md) | 代码地图：模块与文件清单、按问题找代码、文档索引、扫描命令 | 分析 / 定位代码时；**增删或移动文件后必须同步更新** |
| [`docs/prototype.md`](docs/prototype.md) | 原型位置的**唯一入口** + 使用界限（原型结论一律作废） | 想参考原型时 |
| [`docs/plans/`](docs/plans) | **实现方案文档**（一次决策一个文件；写完即冻结，偏差另建文档） | 要写 / 查实现方案时 |

## 四条不可协商的红线

细则见对应文档，此处只列结论：

1. **未讨论不改代码** —— 先讨论 → 把方案写入 `docs/plans/` → 等用户**明确批准** → 才实施。
   → [`docs/spec/workflow.md`](docs/spec/workflow.md)
2. **默认不提交** —— 用户没明说就不执行任何 git 写操作（commit / push / amend / rebase …）；
   授权**只对当次请求生效**，提交完即回到禁止态。
   → [`docs/spec/git.md`](docs/spec/git.md)
3. **中文注释** —— 类 / 接口 / 对象 / 枚举 / 函数 / 结构体**及其成员**必须有中文文档注释。
   → [`docs/spec/comments.md`](docs/spec/comments.md)
4. **规范自持** —— 不直接引用原型内部文档（只经 [`docs/prototype.md`](docs/prototype.md)）；
   原型的结论与测试验证一律作废，必须在本仓库重新验证。
   → [`docs/spec/workflow.md`](docs/spec/workflow.md) §4

## 查资料顺序

**先文档，后源码** —— 问第三方 / 平台 API 的用法时，先用文档查询工具（`find-docs` / Context7）
取官方文档；查不到、版本对不上、或必须确认实现细节时才读源码。
→ [`docs/spec/workflow.md`](docs/spec/workflow.md) §6

## 开工前自检

见 [`docs/spec/workflow.md` §5 实施前自检清单](docs/spec/workflow.md)。
