# `:agent` · 启动编排（F6）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/runtime.md`](runtime.md)（F6）**§3 的顺序部分**。
> **进程引导链与主线程骨架见 [`agent/runtime.md`](runtime.md) §3.1 / §3.2**（不在此重复）；
> 线程划分见 [`agent/runtime-threads.md`](runtime-threads.md)；**日志归 F7**
> （[`agent/runtime-observability.md`](runtime-observability.md) §3，**已定**）。
> **变更记录记在本文件 §6。**

**状态**：**已定**（2026-10-07）。

---

## 1. 顺序

| # | 步骤 | 硬依据 |
|---|---|---|
| 1 | ① 装 `Thread.setDefaultUncaughtExceptionHandler` | [`agent/runtime.md`](runtime.md) §3.2 ①：**引导本身会抛"看不懂的异常"**（不变量 9），必须先有兜底 |
| 2 | ② 进程引导 | 不变量 9 / [`agent/display.md`](display.md) §2.2 |
| 3 | ③ 解析 CLI 参数 | **见 §1.1 的 CLI 契约**：业务参数走协议，CLI 只放**进程级与调试** |
| 4 | 建队列与共享状态 + **构建能力表** + **设备级前置**（解析隐藏成员 → CDM 关联自检 → 建虚拟设备，见 §1.2） | **不变量 5 的前半**；能力表见 [`agent/transport-handshake.md`](transport-handshake.md) §4；前置失败 ⇒ **退出码 2** |
| 5 | ④ 起**进程级**线程：`slow` → **`accept`（必须最后）** | **不变量 5 在此满足**；**每路线程（`reader` / `link` / `sender`）由 `accept` 在接受到连接时起** |
| 6 | ⑤ `Looper.loop()` | [`agent/runtime.md`](runtime.md) §3.2 |

**脚本层不在本文件的编排内**（产物推送、启动命令、stdout 处理）——
**早期日志**由 logcat 天然解决（[`agent/runtime-observability.md`](runtime-observability.md) §3）。

### 1.1 CLI 契约（**改了要同步文档**）

```
app_process / com.xiaowine.wcreen.agent.Main <传输> [选项]
```

| 参数 | 取值 | 缺省 | 说明 |
|---|---|---|---|
| **`<传输>`**（位置参数，**必填**） | `adb` \| `tcp` | — | 决定"监听什么" —— §2 的**鸡生蛋**（监听先于任何连接） |
| `--name=<抽象名>` | 字符串 | `wcreen` | 仅 `adb` ⇒ `LocalServerSocket("<名>")` |
| `--port=<端口>` | 1–65535 | `27183` | 仅 `tcp` |
| `--log=<V\|D\|I\|W\|E>` | 等级 | `V` | **启动期**日志等级；连上后由 PC 配置（也可 `setprop log.tag.<TAG>`） |

- **业务参数一律走协议** —— CLI 只放**进程级与调试** ✓
- **不提供业务调试开关** —— F1–F7 已**没有可调的业务参数**（必须位不可配、心跳与判活固定），
  能加的只剩"绕过核心承诺"（如自定义 flags 组合）⇒ **实验走探针** ✓

### 1.2 启动期的设备级前置（VDM）

建屏改走**虚拟设备**（[`agent/display.md`](display.md) §3），因此启动期多两步**硬前置**，
都在第 ④ 步内、**早于 `accept`**（失败 ⇒ 退出码 2，PC 立刻知道设备不可用）：

| # | 前置 | 取法 | 失败形态 |
|---|---|---|---|
| 1 | **CDM 关联**（缺则建 + 解析 id） | 以 uid 2000 执行 `cmd companiondevice list/associate`（`display/CompanionAssociation.kt`）；**幂等**，用户撤销后下次启动自建 | fatal（`association`） |
| 2 | **建虚拟设备**（进程内**一台**，多路共用） | 反射域 `wrappers/VirtualDevice.kt`（`code-hiddenapi.md` §3 第 7 处）；设备锁态 = `LOCK_STATE_ALWAYS_UNLOCKED` | fatal（`association` / `wrapperMissing`） |

- 引导链另有一条**非反射但同样硬**的要求：伪装 Context 必须覆写 `getApplicationContext()` 返回自身
  —— `VirtualDeviceInternal` 构造时经它取 attribution，而 `app_process` 里没有 Application
  （真系统 Context 返回 null ⇒ NPE，实测踩过）。
- **建屏不在这里**：屏仍由上游的「申请屏幕」指令触发（[`agent/display-lifecycle.md`](display-lifecycle.md) §1）。

---

## 2. 三条硬约束怎么定出这个序

| 约束 | 推出的结论 |
|---|---|
| **异常兜底最先**（`runtime.md` §3.2 ①） | ① 必须早于**一切**，包括进程引导 |
| **不变量 9 / `display.md` §2.2** | 引导（②）必须早于**任何线程** —— `link` 要用伪装 Context 取 `DisplayManager` |
| **不变量 5**（handler 先于 `accept()`） | "就绪" = 路由表 + 各目标队列 + 其它线程；`accept` 既监听又分派 ⇒ **`accept` 只能最后起** |

**唯一必须在启动时给、不能走协议的业务相关参数 = 传输选择** ——
它决定"监听什么"，而**监听先于任何连接**（鸡生蛋）。
其余（`N`、断开后立即释放、编码参数、建屏参数）**全部走协议**。

---

## 3. 线程启动顺序

- **启动时只起进程级线程**：`slow` → **`accept`（必须最后）**；
- **能力表在启动时构建一次**（设备级静态量，全进程不变；需伪装 Context ⇒ 必须在 ② 之后，
  且**早于**任何连接）—— 见 [`agent/transport-handshake.md`](transport-handshake.md) §4.1；
- **每路线程由 `accept` 起** —— 接受到连接时按 `reader` → `link` → `sender` 顺序起（写者先于生产者）；
- **`main` 的 Looper 在 ② 就已 `prepare()`**，但 `loop()` 在 ⑥ 才跑 ⇒
  ②~⑤ 期间别的线程 `post` 到 `main` 的事件**只排队、不会丢**；
- **启动阶段不建屏**（[`agent/display-lifecycle.md`](display-lifecycle.md) §1：agent 不自动建屏）
  ⇒ **启动不占并发额度**。

---

## 4. 残留清理：**不做**

| 曾经的顾虑 | 结论 |
|---|---|
| 孤儿虚拟屏 | **系统随 owner 进程死亡回收**（本机实测，实体机 + AVD 一致）⇒ **无需扫屏名前缀** |
| 孤儿编码器会话 | 随进程死释放（同上） |
| **重复实例** | **由"监听地址被占用"天然判出**（`localabstract` 名 / TCP 端口，[`agent/runtime.md`](runtime.md) §2.1 副作用）⇒ 绑定失败即 fatal，**不需要额外检测步骤** |

**未验风险**：厂商内存管理（如小米 `mimd`）是否会让屏不被回收 —— 出问题再加清理。

---

## 5. 启动阶段的失败与退出码

启动阶段有三类可失败点：**引导失败** / **参数非法** / **监听地址被占用**。
三者都 ⇒ **报 fatal ⇒ 走收尾 ⇒ `System.exit(非 0)`**。

**已锁一条**：**退出码必须能区分"启动失败"与"正常运行后结束"** ——
因为启动失败时唯一的诊断入口就是**退出码**（日志通道尚未建立）。

**已定**：`0` 正常结束 / `1` 运行中致命错误 / `2` **启动失败** —— 见 [`agent/runtime-failure.md`](runtime-failure.md) §4。

---

## 6. 变更记录

- **2026-10-07 定稿**：启动编排落盘。锁定**六步顺序**（①异常兜底 → ②进程引导 → ③解析 CLI 参数 →
  建队列 → ④起进程级线程（`slow` → **`accept` 最后**）→ ⑤`Looper.loop()`）；给出三条硬约束如何定序；
  **传输选择是唯一必须启动时给的业务参数**（监听先于连接）；
  **不做残留清理**（屏随 owner 死被回收 + 重复实例由监听地址占用判出）；
  启动失败 ⇒ fatal ⇒ `System.exit(非 0)`，退出码须能区分启动失败与正常结束。
  **日志（含早期日志）保持待定**，归 F7。
- **2026-10-07 补 §1.1 CLI 契约**：F8 撤销后，它唯一的剩余内容（**CLI 契约**）并入本文件 ——
  `<传输>` 位置参数（必填）+ `--name` / `--port` / `--log`；**业务参数一律走协议**；
  **不提供业务调试开关**（实验走探针）。
- **2026-10-07 VDM 实施（双端实测）**：第 ④ 步内新增**设备级前置**并补 §1.2 ——
  **CDM 关联自检（缺则建，幂等）+ 建虚拟设备（进程内一台，多路共用）**；两者失败均 ⇒ **退出码 2**；
  同时记录引导链的新硬要求（伪装 Context 必须覆写 `getApplicationContext()` 返回自身，
  否则建虚拟设备 NPE —— 实测踩过）。建屏仍由「申请屏幕」触发，**启动阶段不建屏**不变。
