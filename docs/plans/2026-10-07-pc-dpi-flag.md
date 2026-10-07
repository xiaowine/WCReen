# PC 端：新增 `--dpi`（虚拟屏密度）开关

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 定义的实现方案文档。
> 范围：**PC 侧命令行 + 协商透传**（`:agent` 零改动）。
> 写完即冻结；实施中的偏差另建文档。

**状态**：**已批准**（2026-10-07，用户指示「把当前 pc dpi 设置到手机测试下」）。

---

## 1. 背景

- 协议**早已支持** `params.dpi`：agent 的 `Screen.kt` / PC 的 `dto.rs` 都有该字段，
  回报里也有 `applied.dpi`；agent 的规则是 `params.dpi > 0` 用之，否则**回退主屏
  `densityDpi`**（`EncoderParams.resolve`）。
- PC 侧却把它**写死为 `None`**（`negotiate.rs`），命令行也没有开关 ⇒ 用户无法指定密度。
- **本轮实机实测**（2026-10-07）：密度决定 App 看到的 **dp 空间** ——
  `1440×3200@560dpi` = 411×914dp（与手机一致，App 正常铺满）；
  而 `1920×1080@560dpi` 只有 549×309dp，**竖屏锁定的 App 被 Android 关进 253×309dp 的
  小屏兼容窗口**（`letterBoxed=true`、两侧黑边、内容极少）。
  ⇒ 密度是「一块屏能装多少内容」的旋钮，必须可配。

## 2. 方案

| 项 | 决定 |
|---|---|
| CLI | 新增 `--dpi <DPI>`，类型 `Option<u32>`，缺省 `None`（= 由 agent 推导 = 主屏密度） |
| 校验 | 仅要求 `> 0`。**不做范围校验** —— 能力表里没有 dpi 的任何范围数据，凭空造一个范围就是**发明约束** |
| 透传 | `Args.dpi → negotiate::Request.dpi → ScreenParams.dpi`，与 `--width/--height` **完全同路** |
| 生效时机 | 与尺寸/帧率一样**只在建屏 / 重连时下发** ⇒ 改 dpi 需要重连（重启 PC 客户端；断开即拆屏，agent 无需改动） |
| 可见性 | 不新增日志字段 —— **agent 已在 `applied.dpi` 回传实际生效值**；验收用设备侧 `wm density -d <id>` 独立佐证 |

## 3. 影响面

| 区域 | 变化 |
|---|---|
| `pc/app/src/cli.rs` | `Args.dpi` 字段 + `--dpi` 解析分支 + 帮助文本 + 3 项单测 |
| `pc/app/src/negotiate.rs` | `Request.dpi` 字段；`params.dpi = req.dpi`；模块文档表格补一行「密度」 |
| `pc/app/src/main.rs` | 组装 `Request` 时透传 |
| `:agent` | **零改动** |
| `docs/code-map.md` | 无需改（未新增/删除文件） |

## 4. 验证方式

| 层 | 手段 |
|---|---|
| 单测 | 缺省为 `None`；`--dpi 240` / `--dpi=480` 两种写法；`--dpi 0`、缺取值、`--dpi=abc` 均报错 |
| 构建 | `cargo build --release --workspace` 零告警；`cargo test --workspace` 全绿 |
| **实机 ①** | `--dpi 560`（= 手机密度）⇒ 屏密度 560、配置 `sw411dp`，与不传参数**逐项一致** |
| **实机 ②** | `--dpi 320` ⇒ 屏密度变 **320**、`sw720dp`（1440 ÷ (320/160) = 720），画面布局随之变化（原始帧缓冲佐证） |
| **实机 ③** | 再切回 560 收尾，现场回到「与手机一致」 |

## 5. 风险与回滚

- 传**极端 dpi**（如 1 / 10000）⇒ App 的 dp 空间会荒谬。但那是**用户显式要求**，
  且 `applied.dpi` 会回传、`wm density` 可自查；不加人为范围（与 `--width/--height`
  的「显式即硬要求，不静默修正」一致）。
- 回滚：删掉开关即回到「由 agent 推导」，无协议影响、无残留。

---

## 6. 变更记录

- **2026-10-07 交付并实机验收**（用户指示「把当前 pc dpi 设置到手机测试下」）：
  - `--dpi` 落地：`cli.rs`（字段 + 解析分支 + 帮助文本 + 3 项单测）、
    `negotiate.rs`（`Request.dpi` + 规则表补「密度」一行 + `params.dpi = req.dpi`）、
    `main.rs`（透传）。**`:agent` 零改动**（字段与回退规则早已实现）。
  - 构建 `cargo build --release --workspace` **零告警**；`cargo test --workspace` ——
    **app 24 项**（21 + 新 3）/ **proto 23 项** 全绿。
  - **实机三步**（各步均以设备侧 `wm density -d <id>` 独立佐证）：
    ① `--dpi 560` ⇒ 屏 1440×3200 @ **560** ✓（与「不传参数」一致）；
    ② `--dpi 320` ⇒ 屏 @ **320** ✓，B 站 配置由 `sw411dp … 560dpi nrml` 变为
       **`sw720dp w720dp h1600dp 320dpi xlrg`**，窗口仍**满屏** ⇒ **一屏内容明显变多**
       （画面切到平板/XL 布局：顶部搜索栏消失、卡片更密）✓；
    ③ 切回 `--dpi 560` ⇒ `sw411dp w411dp h914dp 560dpi nrml` ✓，与 ①/默认**逐项一致**。
  - 结论：**密度是「一块屏装多少内容」的旋钮**，已可由 PC 命令行控制 ✓。
  - 文档：`tech-stack/pc.md` / `pc-frame-path.md` 不列 CLI 开关（开关的契约就在 `cli.rs` 的
    帮助文本里，且已更新）⇒ **无需同步**；`code-map.md` 无新文件 ⇒ 无需改。
