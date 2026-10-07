# `:agent` 模块规范变更记录

> 本文件是 [`agent/module.md`](module.md) 的**变更记录**，从该文件拆出（原文件超单文件上限）。
> 规范正文见 [`agent/module.md`](module.md)；功能状态见 [`agent/features.md`](features.md)。

---

- **2026-10-06 初版**：定位、硬约束（8 条不变量）、依赖与构建约束、构建部署、
  对外契约（未定稿）、验证关注点、禁忌清单。
- **2026-10-06**：新增「子系统划分与推进顺序」—— 按 [`docs.md`](../docs.md)
  把 `:agent` 拆为 4 个子系统文档（**逐个讨论、逐个定稿**）；其余小节顺延编号。
- **2026-10-06**：子系统章节收敛为**结构映射**（子系统 → 文档）；**功能级状态与推进顺序移交**
  [`agent/features.md`](features.md)（单一事实来源），避免同一状态维护在两处。
- **2026-10-06**：子系统 #4 的文档由单一 `agent-input-launch.md` **拆为两篇**
  （一个文件一件事）：**F2 → [`agent/launch.md`](launch.md)（已定）**、F5 → `agent/input.md`（待建）；
  待建文档名统一为文件夹形式（`agent/runtime.md`、`agent/transport.md`）。
- **2026-10-06 拆分**：F3 文档改为**入口 + 6 个子文档**（见 [`agent/encode.md`](encode.md) §2 文档地图）；
  子系统表的"虚拟屏与编码"行同步更新。
- **2026-10-06 实机验证**：不变量新增第 9 条（Context 引导 + `DESTROY_CONTENT_ON_REMOVAL`），
  「依据」列新增 `本机实测`；F1 验收 1–5 成立、F2 六项待验全部有结论。
- **2026-10-06 决定**：`DESTROY_CONTENT_ON_REMOVAL` 改为**固定必须位**（原为 PC 可配）——
  依据是 2×2 对照实验：关掉后拆屏与崩溃两条路径都会污染本机屏。不变量第 9 条已同步。
- **2026-10-06**：子系统 #1 的文档 [`agent/runtime.md`](runtime.md) **创建**（F6 + F7 骨架落盘）；
  子系统表的"运行时"行同步 —— 边界补「启动方式（脱离 adb）」与「退出与收尾」。
- **2026-10-07 契约收口**：§6 的**四项全部收口** —— **部署路径**定为 **`/data/local/tmp/`**
  （`wcreen-agent.jar` + `run-agent.sh`；**无日志文件**，日志走 logcat）；
  **控制脚本格式撤销**（控制通道由协议承担，`run-agent.sh` 只负责启动）；§5 的部署示例同步。
  连同此前的「线协议」「命令行」，**§6 四项已全部有结论**。
- **2026-10-07**：§6 对外契约 —— 「线协议」与「命令行」两行由**未定稿 / 进行中**转**已定**
  （F8 撤销后，CLI 契约并入 [`agent/runtime-startup.md`](runtime-startup.md) §1.1）。
- **2026-10-07 拆分**：**本变更记录整段提成独立文件**（原文件超单文件上限）；
  `module.md` 只留一行指针。
- **2026-10-07 VDM 实施**：§3 硬约束新增一条 —— **屏挂在虚拟设备上** ⇒ 启动时需要一条
  **self-managed 的 CDM 关联**（包名 `com.android.shell`、profile `…APP_STREAMING`），
  agent 启动自检、缺则自建（`display/CompanionAssociation.kt`，走 `cmd companiondevice`，**不反射**）；
  用户撤销后下次启动重建（幂等）。双端实测通过（真机 16 全链路 + AVD 15 建屏）。
