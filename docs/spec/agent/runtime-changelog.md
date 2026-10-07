# `:agent` 运行时 · 变更记录

> 本文件是 [`agent/runtime.md`](runtime.md) §10 的正文（该段因超出单文件上限已提成独立文件）。
> 运行时的**当前规范以 [`agent/runtime.md`](runtime.md) 及其子文档为准**，本文件只记历史。

---

- **2026-10-06 初版**：F6 / F7 骨架落盘。**已定**：**脱离 adb 启动**（本机实测依据）、
  **不做保活**（`oom_score_adj = -1000` 实测）、主线程骨架（`Looper.loop()` + 完成计数 ⇒
  `quitSafely()` ⇒ `finally` 收尾 + `System.exit`）、异常兜底最先装、**日志走协议**、
  **判活看心跳不看帧**、退出触发集合与收尾硬顺序。其余显式标「待定」。
- **2026-10-07 传输无关性**：新增 §2.1.1 —— **启动与通信解耦**：启动**只能走 adb**（uid 2000），
  通信**可插拔**（adb / 网络直连，未决项 9）⇒ 脱离是两种传输的共同前提，网络模式下可**拔线续流**；
  §2.2 补网络模式后果（崩了只能重新插线，`:app` 看门狗属后续）；
  §7 加前置约束「线程模型必须传输无关」。与 [`tech-stack/overview.md`](../tech-stack/overview.md) §5 未决项 8 对齐。
- **2026-10-07 线程模型定稿**：§7 由「待定」转**已定**并**整段提成 [`agent/runtime-threads.md`](runtime-threads.md)**
  （原文件超单文件上限）：1 主 + 5 固定角色线程、每状态一个 owner、不设业务锁、消息路由表、
  传输接缝（连接方向 = agent 监听 / PC 主动连）、零拷贝与背压、心跳零新线程、收尾与 join 顺序。
  §6 的「完整动作序列」同步收口（只剩退出码 / 信号处理待定）。
- **2026-10-07 生命周期定稿**：§8 由「待定」转**已定**并**整段提成 [`agent/runtime-lifecycle.md`](runtime-lifecycle.md)**：
  不搞全局状态机（两个生命周期正交）、显示链路 3 态、合法组合表 + 4 条不变量、
  协调者只持会话级策略与终局、仲裁由队列串行化、命令幂等、线程意外退出 = fatal。
- **2026-10-07 启动编排定稿**：§3.3 由「待定」转**已定**并提成 [`agent/runtime-startup.md`](runtime-startup.md)
  （六步顺序、`accept` 最后、不做残留清理、启动失败退出码）；**日志仍待定**（§4）。
- **2026-10-07 失败与退出定稿**：§9 由「待定」转**已定**并提成 [`agent/runtime-failure.md`](runtime-failure.md)
  —— 四级按「谁负责善后」划分、**致命来源只有三类**、子系统级判据 = 能力缺失、可恢复的重试由 PC 负责、
  不做自动升级、退出码 `0`/`1`/`2`、**接管 `SIGTERM`**（含死锁规避）、收尾超时值。
  §6 同步收口；§2.1 明确「**结束会话**」与「**停止 agent**」是两条不同指令。
- **2026-10-07 心跳定稿 + 变更记录提成**：§5 转**已定**并提成
  [`agent/runtime-observability.md`](runtime-observability.md)（**周期固定 10s**、**心跳是独立消息
  不承载协议内容**、**超时判据 = `setSoTimeout(30s)`**）；§4 日志转指针；
  **§10 变更记录提成 [`agent/runtime-changelog.md`](runtime-changelog.md)**（原文件已满 200 行）。
- **2026-10-07 日志定稿**：§4 由「待定」转**已定** —— **通道 = 复用 adb logcat（`android.util.Log`）**，
  实测：能写 / **脱离启动也能写**（走 logd socket，不走 stdout）/ 任意线程能写 / **不阻塞**（20000 条 59ms）；
  三项待定**全部收口**（连接前日志天然解决 / 等级与限流交给 Android 原生 + `setprop log.tag.<TAG>` /
  **不落盘**）。**协议日志通道后置**（等 F4 评估）。
- **2026-10-07 多路结构修订（F4 议题① 回改）**：运行时改为**单进程多路** ——
  **一路 = 一块屏 = 一条连接**；线程归属分化（`main` / `accept` / `slow` 进程级；
  `reader` / `link` / `sender` 每路）；**`control` 拆为进程级 `accept` + 每路 `reader`**；
  §3.3 启动编排由「七步」改**六步**（启动时只起进程级线程，每路线程由 `accept` 起）；
  §7 线程模型摘要、§6 收尾（泛化为「对每一路」）同步更新。
  详见 [`agent/transport-topology.md`](transport-topology.md)。

---

## 线程模型（由 [`agent/runtime-threads.md`](runtime-threads.md) §10 提成）

- **2026-10-07 定稿**：线程模型落盘 —— **1 主 + 5 固定角色线程**（`main` / `link` / `sender` /
  `control` / `slow`，F5 再加 `input`），**每状态一个 owner、全模块不设业务锁**；含消息路由表、
  各线程循环、**传输接缝**、**零拷贝与背压**（唯一共享点）、**心跳零新线程**、**收尾与 join 顺序**。
- **2026-10-07 实测**：§6 的「跨线程 `releaseOutputBuffer`」由**待验证**转**已实测安全**
  （785 / 786 / 110 帧，0 异常 0 泄漏）⇒ **退路不需要**；**日志改走 logcat** ⇒ §2.1 / §4 论证改为
  "**心跳与控制应答**"；§5 传输 API 转**已实测**（`shutdown*` 可用、**一个监听名可同时接受多条连接**、
  `setSoTimeout` 抛 `IOException("Try again")` ⇒ 判活改用 `Os.poll`）。
- **2026-10-07 多路结构修订**：线程模型改为**单进程多路**（一路 = 一块屏 = 一条连接）——
  线程表加「归属」列（`main` / `accept` / `slow` 进程级；`reader` / `link` / `sender` **每路**）；
  **`control` 拆为进程级 `accept` + 每路 `reader`**；§8 收尾序列泛化为「对每一路」；形状图改为多路形态。
- **2026-10-07 S1 实测修正（三处规范与实测不符）**：
  ① **`java.net.Socket` 没有 `getFileDescriptor()`** ⇒ 判活按传输分两种机制
  （见 [`agent/runtime-observability.md`](runtime-observability.md) §2.1），「两种传输同一实现」撤销；
  ② **`LocalServerSocket.close()` 不会叫醒阻塞中的 `accept()`** ⇒ 传输接缝新增 `wake()`
  （自己连自己一次），收尾序列 ① 改为「停止标志 + `wake()` → join → `close()`」，
  实测收尾从 **3001ms 降到 101ms**；
  ③ **`SIGTERM` / `SIGINT` 不触发 shutdown hook** ⇒ 见 [`agent/runtime-failure.md`](runtime-failure.md) §5。
  另：**控制队列深度定为 64**（`runtime-threads.md §9` 的待定项收口）。
- **2026-10-07 S4 输入注入落地（F5）**：`input` 线程实装 —— **每路一条**、线程表第 **6** 条角色、全进程**第 7** 条线程
  （`Thread("input")`，见 [`agent/runtime-threads.md`](runtime-threads.md) §2）；独占**注入队列**（合并策略
  `move` 覆盖 / `scroll` 累加，队列容量 **8**、不可合并事件硬上界 **64**），**不与 `slow` 共用**（§2.1）。
  §8 收尾序列**第 ③ 步**新增「停 `input` → join」，且**早于第 ④ 步的 `link`** —— 输入线程必须先于屏释放退出，
  否则可能往已拆的屏上注入；**连接断开即停**（与 `link` 同理，防线程泄漏）。
  协议侧：`input` 指令（`action` + `pointers[]` / `scroll` / `key`）、新错误码
  `noScreen` / `badInput` / `inputDenied` / `inputFailed`、`EventMsg` 增 `subsystem` 字段；
  **成功注入不回 ack**（高频静默，仅失败回）。
  取法（实机 Android 16 / SDK 36）：`getSystemService(Context.INPUT_SERVICE)` **零反射**取 `InputManager`；
  反射**恰好 3 个成员**（`InputManager.injectInputEvent(InputEvent,int)` / `InputEvent.setDisplayId(int)` /
  常量 `INJECT_INPUT_EVENT_MODE_ASYNC`），事件本体全走公开 API（`MotionEvent.obtain` 14 参公开重载 /
  `KeyEvent` 公开构造器）。实测纠错：注入模式常量实为 `ASYNC=0 / WAIT_FOR_RESULT=1 / WAIT_FOR_FINISH=2`
  （原表误记 `WAIT_FOR_FINISH=1`，已改 2）。
  实机验收 **31 项全过**（滑动 ⇒ 虚拟屏原始截图 **91.9%** 行变化、滚轮 **91.8%**、本机屏 **0.000%**；
  BACK ⇒ 画面 100% 变化且该屏前台转无；`stopAgent` 退出码 **0**）；单测 **60 项全绿**（新增 **18**）。
  §2 线程表无「本片 / 未实现」状态列 ⇒ **未增改**；§8 第 ③ 步原文「通知 input → join」已与实现一致 ⇒ **未增改**。
