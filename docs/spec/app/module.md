# `:app` 模块规范

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一。
> **改动 `app/` 下任何内容之前必须读本文件。**

---

## 1. 模块定位

**激活器 App**（占位，尚未实现）。App 进程（普通 uid），技术栈为 **Kotlin/JVM**，
与 `:agent` 同工具链、同版本下限。职责：

- **自激活**（配对 ADB / Shizuku 等外部机制）；
- **无线配置**：把连接方式从 USB + adb forward 解放出来（依赖协议与传输的传输无关性，见 [`tech-stack/overview.md`](../tech-stack/overview.md) §5）；
- 推送 `:agent` 产物与启动脚本到设备；
- 以 shell 身份拉起 agent；
- 起看门狗保持存活。

**虚拟屏逻辑不在本模块** —— 那部分在 `:agent`（跑在 shell 进程里）。见 [`agent/module.md`](../agent/module.md)。

⚠️ **App 拿不到 shell**：App 自己写不了 `/data/local/tmp`（属 `shell:shell 0771`），也没法以 uid 2000 起进程。
所以激活器**必须**借助外部机制。

---

## 2. ⚠️ 未定分叉：激活路线

这是**成本差一个数量级**的分叉，**必须先与用户定**再动代码。

| 方案 | 代码量 | 体积 | 用户的一次性操作 |
|---|---|---|---|
| **Shizuku** | ~150 行 | 0 | 装 Shizuku 并激活 |
| **内置 adb 二进制**（LADB 式） | ~1000 行 | +5 MB | 在 App 里输无线调试配对码 |
| **只做 PC 侧激活** | 0 | 0 | 每次插 USB（即当前流程） |

Shizuku 路线用 `Shizuku.newProcess(...)` 以 shell uid 起 `app_process`，正好对应 `:agent` 既有的启动方式。
内置 adb 路线要自己实现 ADB 协议 + TLS + 配对流程（Android 11+ 的 adbd 走 TLS，且 TLS 模式下 `A_AUTH` 被忽略）。

---

## 3. 当前技术参数

来自 `app/build.gradle.kts`：

| 项 | 值 | 说明 |
|---|---|---|
| 插件 | `com.android.application` | 常规可安装 App |
| namespace / applicationId | `com.xiaowine.wcreen` | — |
| minSdk | **34** | 与 `:agent` 一致，避免两套下限 |
| targetSdk / compileSdk | 37 | 与 `:agent` 一致 |
| Java 级别 | 21 | 与 `:agent` 一致 |
| 依赖 | **零依赖** | 做 UI 时再按需加 androidx / material |
| release 构建 | R8 裁剪关闭 | 与 `:agent` 相反 |

**版本下限与 `:agent` 保持一致**（minSdk 34 / targetSdk 37 / compileSdk 37 / Java 21）：
两个模块面向同一批设备，避免出现两套下限导致构建与心智分裂。
Android 14（API 34）是 `TRUSTED` 等虚拟屏 flag 可用的下限。改动任一值需先讨论。

---

## 4. 构建

```bash
./gradlew :app:assembleDebug      # → app/build/outputs/apk/debug/app-debug.apk
./gradlew :app:assembleRelease    # → app/build/outputs/apk/release/app-release-unsigned.apk
```

---

## 5. 约定

- 本模块的**行为契约**（它如何调用 agent、传什么参数）一旦确定，须写入本仓库的 `docs/plans/` 方案文档。
- 新增源码放到 `app/src/main/kotlin/com/xiaowine/wcreen/`（**不是 `java/`**）。
- 新增文件后同步更新 [`docs/code-map.md`](../../code-map.md)。
- 注释遵循 [`comments.md`](../comments.md)；工作流遵循 [`workflow.md`](../workflow.md)。
- 若最终决定不实现激活器，本模块**整体删除**（含 `settings.gradle.kts` 里的 `include(":app")`）——
  不要留空壳。见 [`workflow.md §4 交付底线`](../workflow.md)。
