# `:agent` · 枚举可启动应用（F2 · 枚举指令）

> 本文件是 [`AGENTS.md`](../../../AGENTS.md) 索引下的规范之一，属 [`agent/launch.md`](launch.md)（F2）的**指令细节**。
> F2 的**范围、指令模型（共同约束）与「打开目标应用」**见 [`agent/launch.md`](launch.md)。

---

## 1. 请求

| 参数 | 可否缺省 | 说明 |
|---|---|---|
| `forceRefresh` | 可（缺省 `false`） | **获取参数，不是独立指令**：`true` 时绕过缓存重新查询 |

**可在任意时刻下发，且不依赖屏**（无屏也能用，见 [`agent/launch.md`](launch.md) §2）。

---

## 2. 实现路径（已定）

走**进程内** `IPackageManager.queryIntentActivities` + `ApplicationPackageManager`，**不走** `cmd package` 子进程。

理由（已实测）：`cmd package query-activities` 只给包名与组件名，**不给 label**；
`dumpsys package` 的 `nonLocalizedLabel` **实测全为 null**。没有 label 的名单对 PC 用户不可用。

基础设施（伪造 `ActivityThread` 拿系统 Context、包名伪装 `com.android.shell`）是
**F1 建屏已必须有的**，本功能**增量≈0**；其具体约束见 [`agent/display.md`](display.md) §2.2。

**已实测可行**：`app_process` 下 `ApplicationPackageManager.getApplicationInfo().loadLabel()`
返回本地化文本（设置 / 浏览器 / 夸克 / 支付宝），**连被禁用的包也能取到**。

---

## 3. 返回字段（最小集）

| 字段 | 说明 |
|---|---|
| 包名 | |
| 入口组件名 | `包名/组件名` |
| label | 展示名；取不到时退回包名 |
| 是否系统应用 | 供 PC 分组 / 过滤；判据 = `ApplicationInfo.flags & FLAG_SYSTEM`（已实测系统应用置位、第三方不置位） |

**图标不下发**（v1）：入口数 × 图标 ≈ 1–3 MB，且取图标需为每个包建 `Resources`，是秒级开销。
预留"按需拉取 + PC 侧缓存（key = 包名 + 图标 hash）"，v1 用 label 首字母占位。

---

## 4. 范围与缓存

- **只列** `android.intent.action.MAIN` + `android.intent.category.LAUNCHER` 入口。全部已安装包作为**调试开关**，缺省关。
- agent 侧**缓存** PackageManager 查询结果；`forceRefresh` 时重查。
- **无广播订阅**：`app_process` 下注册 `BroadcastReceiver` 不可行 ⇒ **TTL + 强制刷新**是唯一的失效手段。
- **只做当前用户**，不做跨用户。

### 4.1 已实测的名单口径（本机 Android 16 / SDK 36）

| 事实 | 观察值 |
|---|---|
| 默认口径（`MAIN`+`LAUNCHER`） | **84 条**入口 |
| 已安装包总数 | 471 |
| 其中「第三方且有入口」 | 70 条 |
| **被禁用（`enabled=2`）的包** | **不进默认名单**（3 个禁用包命中 0）；加 `MATCH_DISABLED_COMPONENTS`(512) 后 84 → **192**（多出的多数是**已启用包里的禁用 Activity**，不只是禁用包） |
| 禁用包的 label | **仍能取到**（`com.quark.browser` → 「夸克」）⇒ label 与"是否入名单"是两件事 |

> ⚠️ 若以后要放开 `MATCH_DISABLED_COMPONENTS`：**"能列出"不等于"能拉起"** ——
> 禁用包拉起必失败（见 [`agent/launch.md`](launch.md) §5.1）。

---

## 5. 变更记录

- **2026-10-06 初版**：从 [`agent/launch.md`](launch.md) 拆出（原文 §3）——
  该文件在回填实机验证结果后超出 [`docs.md`](../docs.md) §2 的单文件上限，
  按"内聚子主题整段提成同级文件"拆分。
- **2026-10-06 实测回填**：`forceRefresh` 语义、进程内路径可行性、字段判据、名单口径（含禁用包）。
