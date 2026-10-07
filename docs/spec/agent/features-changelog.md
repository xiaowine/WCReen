# `:agent` 功能清单 · 变更记录

> 本文件是 [`agent/features.md`](features.md) §5 的正文（该段因超出单文件上限已提成独立文件）。
> 功能状态与范围**以 [`agent/features.md`](features.md) 为准**，本文件只记历史。
> **2026-10-06 及更早的条目已归档** ⇒ [`agent/features-changelog-archive.md`](features-changelog-archive.md)。

---

- **2026-10-07 F6 收口**：**F6 全部子项定稿** —— 启动方式、编排、线程模型、生命周期状态与协调、
  **失败与退出**（[`agent/runtime-failure.md`](runtime-failure.md)：四级按「谁负责善后」划分、
  致命来源只有三类、子系统级判据 = 能力缺失、可恢复的重试由 PC 负责、不做自动升级、
  退出码 `0`/`1`/`2`、接管 `SIGTERM`）。
- **2026-10-07 F8 撤销**：**「参数入口与调试开关」整项去掉** —— 它要做的三件事都被别处吸收完了：
  **参数集**由 `applyScreen` 承载、**调试开关**决定不加、**CLI 契约**并入
  [`agent/runtime-startup.md`](runtime-startup.md) §1.1。**编号 F8 不再复用**（历史引用保持有效）。
  连带：`launch.md §8` 的"交 F8 收纳"改为「**实现常量，不暴露给 PC**」；
  `module.md §6` 的「命令行」与「线协议」两行转**已定**。
- **2026-10-07 协议补缺**：**新增 `setPolicy` 指令** —— 承载 `N`（连续接入失败次数，缺省 5）与
  「断开后立即释放」（缺省关）。**原缺口**：[`agent/display-lifecycle.md`](display-lifecycle.md) §2
  写的是"PC 可配"，但**消息清单里没有承载它们的消息** ✗。**必须是指令而非握手字段** ——
  「断开后立即释放」的典型用法是 PC 关闭前临时打开 ⇒ 要能**在任意时刻下发** ✓
  ⇒ 指令数 **7 → 8**，见 [`agent/transport-messages.md`](transport-messages.md) §2.3。
- **2026-10-07 修订（F5 + PC 侧）**：**输入上报改为「双端合并」** —— PC 侧新增
  [鼠标合并与限频](../tech-stack/pc.md)（`move` **定频采样**、上限 = **屏刷新率**；`scroll` **累加**；
  `down`/`up`/`cancel`/`key` **立即发**）⇒ 省协议开销；**同时修正 `agent/input.md` §5 的错误**：
  `scroll` 的合并是**累加**而**不是覆盖**（滚动量是增量语义，覆盖会**少滚**）✓
- **2026-10-07 F5 定稿**：**输入注入已定** ⇒ [`agent/input.md`](input.md)
  —— **四项前提已实机验证**（uid 2000 可注入 / 定向到虚拟屏 / **本机屏不受影响** / 触摸被 App 消费）；
  **事件表示 = 触点快照 + 粗粒度 action**（`pointerIndex` 由 agent 推导）；
  **坐标 = 屏像素**；**注入模式 = `ASYNC`**（`WAIT_FOR_FINISH` 实测 **10089 ms**，**硬禁用**）；
  **队列容量 8 + 队尾同类型 `move`/`scroll` 覆盖 + `down`/`up`/`cancel` 绝不丢** ⇒ 最坏延迟 ~3 ms。
  顺带**收口 `display-flags.md §6.4` 的四项 flag**：`ALWAYS_UNLOCKED` ✅ 明确生效；
  **`OWN_FOCUS` 与 `STEAL_TOP_FOCUS_DISABLED` 均未观察到效果**（原"本机屏不失焦"假设**被推翻**，
  `OWN_FOCUS` 保留但理由降级）；另记一条**已知行为**（虚拟屏有前台 App ⇒ 物理键盘输入会进虚拟屏）。
- **2026-10-07 F4 议题④ 定稿 → F4 收口**：**消息清单已定** ⇒ [`agent/transport-messages.md`](transport-messages.md)
  —— **9 个类型**、**7 条指令**（`cmd` 区分）；`applyScreen` 承载全部建屏参数，
  回报携带 **`applied` 实际生效值**且**允许部分成功**（`ok:false` + `applied`）；
  主动上报 3 种事件（**`screenReleased` 只在 agent 主动拆屏时发**，PC 发的 `endSession` 由回报覆盖）；
  错误对象含**分级**（`class`）与机器可读 `code`；**`streamId` 只在 `applyScreen` 与其回报里出现**。
  **F4 五个议题全部定稿**（`input` 待 F5 补）。
- **2026-10-07 F4 议题⑤ 定稿**：**背压与丢帧已定** ⇒ [`agent/transport-backpressure.md`](transport-backpressure.md)
  —— **两个机制分开**（队列有界 + 入队非阻塞 + 丢帧立即 release ⇒ **保护 App**；
  `T_drop` = 200ms + 丢弃模式 ⇒ 画面新鲜度与参考链）；**丢帧只能"丢到 IDR"**；
  **四个动作**（`link` 入队失败 / `sender` 超时或丢弃模式 / 发到 IDR 退出 / `link` 限流请求 IDR）；
  **主动请求 IDR 是必须的**（静止屏上周期 IDR 不来）；容量 16 是内存上限；
  **不加写看门狗** —— 与判活超时同作用 ⇒ **心跳 10s→3s、`T_dead` 30s→10s**；
  **撤销「握手期独立超时」**（沿用 `T_dead`）。
- **2026-10-07 F4 议题③ 定稿**：**协议格式已定** ⇒ [`agent/transport-protocol.md`](transport-protocol.md)
  —— 统一框架 `[类型][长度][负载]`（头定长、上限 16 MiB、头与数据分开写 ⇒ 帧零拷贝）；
  帧负载 = `flags + pts + Annex-B`；控制消息 = **CRC32 + UTF-8 JSON**（CRC 覆盖
  `[类型][长度][负载其余]`，防**失步**）；**引入 Gson**（零依赖政策唯一例外，**已实测**可用）；
  类型表只 **9 个值**（"指令"占一个，指令名在 JSON 的 `cmd` 里）；**不做分片**；字节序小端。
- **2026-10-07 F4 议题② 定稿**：**握手与能力协商已定** ⇒ [`agent/transport-handshake.md`](transport-handshake.md)
  —— 握手只做**版本 + 能力表**（状态操作全走指令 ⇒ **重连与首连同构**）；版本**精确匹配**；
  **能力表启动时构建一次**、无 `forceRefresh`、**不含动态额度**（准入控制 = 直接试）；
  **交集由 PC 算** + 首帧硬件检查；**`streamId` 移到「申请屏幕」**；
  **解码配置不需要独立机制** + 码流 Annex-B；握手超时 10s。
- **2026-10-07 F4 议题① 定稿**：**拓扑与传输已定** ⇒ [`agent/transport-topology.md`](transport-topology.md)
  —— **单进程多路**（一路 = 一块屏 = 一条连接；每路一个进程在网络模式下不可行）；
  引入 **`streamId`**（agent 分配、PC 记忆、重连带回）作为冻结 / 重连的前提；
  **线程归属**（`main` / `accept` / `slow` 进程级；`reader` / `link` / `sender` 每路 ⇒ `control` 拆分）；
  **`N` 次失败 = 断开或接入尝试失败**，**重连成功即清零**（USB 抖动不累积；「断开后立即释放」= `N=1`）；
  同 `streamId` 冲突 ⇒ **新连接取代旧连接**。**回改 F6 三处**（`runtime-threads.md` §2/§4/§8、
  `runtime-lifecycle.md` §4/§5.1）。F4 入口 [`agent/transport.md`](transport.md) 同步建立。
- **2026-10-07 传输 API 实测（F4 前置）**：**一个 `localabstract` 监听名可同时接受多条连接**
  —— 设备内 **3/3**、**经 `adb forward` 3/3**（对端 uid=2000）⇒ **多路只需一个监听地址**；
  `shutdownInput`（读侧立刻 EOF、**写侧仍可用**）与 `shutdownOutput`（对端 EOF）**均可用** ⇒ 收尾技巧成立；
  `setSoTimeout` **有效但抛 `IOException("Try again")`**（非 `SocketTimeoutException`）⇒
  判活改用 **`Os.poll(fd, T_dead)`**（公开 API、两传输同一实现，实测无数据 0/701ms、有数据 1/0ms）；
  另实测：**重复实例 = 监听地址被占用** 可靠检测到活着的实例。
- **2026-10-07 F7 收口**：**日志通道定为「复用 adb logcat」（`android.util.Log`）** —— 实测能写、
  **脱离启动也能写**、任意线程能写、**不阻塞**（20000 条 59ms）；三项待定**全部收口**
  （连接前日志天然解决 / 等级与限流交给 Android 原生 / **不落盘**）。**F7 全部已定**
  ⇒ [`agent/runtime-observability.md`](runtime-observability.md)。
- **2026-10-07 心跳定稿**：**周期固定 10s**、**心跳是独立消息不承载协议内容**、
  **超时判据 = `setSoTimeout(30s)`**（判据 = `T_dead` 内无任何消息；判死只冻结不拆屏）
  ⇒ 见 [`agent/runtime-observability.md`](runtime-observability.md)。
  **日志三项仍待定**。变更记录提成 `agent/runtime-changelog.md`（`runtime.md` 已满 200 行）。
- **2026-10-07 实测**：**跨线程 `releaseOutputBuffer` 已实测安全**（`runtime-threads.md` §6 的唯一
  未验证假设）—— 三组配置（EGL 立即释放 **785** 帧 / 释放延时 20ms **786** 帧 / **真实链路**
  （虚拟屏零拷贝 + DPI 泵帧）**110** 帧）全部 **0 异常、0 泄漏、0 卡死**；release 发起时 dequeue
  正阻塞在 codec 内 **415 / 689 / 89** 次 ⇒ 并发确凿。**退路（拷进自管缓冲池）不需要**。
- **2026-10-07 启动编排定稿**：**F6 的启动编排已定**，提成 [`agent/runtime-startup.md`](runtime-startup.md)
  —— 六步顺序（①异常兜底 → ②进程引导 → ③解析 CLI 参数 → 建队列 → ④起进程级线程 → **`accept` 最后** →
  ⑤`Looper.loop()`）；**传输选择是唯一必须启动时给的业务参数**；**不做残留清理**；启动失败 ⇒
  `System.exit(非 0)` 且退出码须能区分启动失败与正常结束。**日志（含早期日志）仍待定**（F7）。
- **2026-10-07 生命周期定稿**：**F6 的生命周期状态与协调已定**，整段提成
  [`agent/runtime-lifecycle.md`](runtime-lifecycle.md) —— 不搞全局状态机（屏与连接两个生命周期**正交**）；
  **显示链路（屏 + 编码器）合成一个 3 态状态机**（无屏 / 就绪 / 冻结）+ 合法组合表 + 4 条不变量；
  协调者 `main` 只持会话级策略与终局、**不镜像子系统状态**；仲裁由各 owner 的串行队列完成；
  **命令幂等**、**线程意外退出 = fatal（不做部分存活模式）**。
- **2026-10-07 线程模型定稿**：**F6 的线程模型已定**，整段提成 [`agent/runtime-threads.md`](runtime-threads.md)
  —— **1 主 + 5 固定角色线程**（`main` / `link` / `sender` / `control` / `slow`，F5 再加 `input`），
  每状态一个 owner、**全模块不设业务锁**；含消息路由、传输接缝、零拷贝与背压、收尾与 join 顺序。
- **2026-10-07 传输无关性**：F4 的一行目标补「**传输可插拔（adb / 网络，须传输无关）**」——
  启动只能走 adb，通信可换传输；细则见 [`agent/runtime.md`](runtime.md) §2.1.1。
- **2026-10-07 S1 交付（F6 / F7 / F4 连接层）**：**F6 与 F7 转「已实现」**；**F4 转「已定 · 部分已实现」**
  （交付拓扑 / 握手 / 心跳 / 停止，**帧路径随 F1+F3 在 S2**）。实机验收 19 项全过：握手（版本 1 + `state=ready`）、
  心跳周期 **3.00s**、CRC 失步只断单路、单路断开不杀进程、`stopAgent` 退出码 **0**、重复实例退出码 **2**、
  收尾耗时 **101ms**。§4 明确区分**讨论顺序**与**实施顺序**（后者 F4 连接层最先）。
  三处规范按实测修正（`java.net.Socket` 无 `getFileDescriptor` / `close()` 叫不醒 `accept` /
  `SIGTERM` 不触发 hook），见 [`agent/runtime-changelog.md`](runtime-changelog.md)。
  **§3 第 5 处（输入注入取法）的实测时点由 S1 改为 S4**（S1 不含 F5）。
- **2026-10-07 S4 交付（F5 输入注入）**：**F5 转「已定·已实现」**。代码新增 `input/` 包 4 文件
  （`InputCmd`/`InputQueue`/`PointerTracker`/`EventBuilder`）+ `wrappers/InputInjector` + `runtime/InputLoop`
  + `proto/Input`。取法：**零反射**取 `InputManager`，仅反射 **3 个成员**
  （`injectInputEvent`/`setDisplayId`/`MODE_ASYNC`）。实机 **31 项全过**：滑动 **91.9%** / 滚轮 **91.8%**
  行变化、**本机屏 0.000%**、BACK **100%** 且前台消失、`input` 线程无泄漏、退出码 **0**。
  单测 **60 项全绿**、零警告。**纠错**：`WAIT_FOR_FINISH=2`（原记 **1**）。
- **2026-10-07 S4 续（PC 侧 F5）**：新增 `pc/app/src/input.rs` + 协议 `Command::Input`；真实鼠标拖动 ⇒ `输入 11 条`、虚拟屏 96.0% 变化；Escape ⇒ 前台清空；33 项单测零警告。
