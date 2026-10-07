# 调查：虚拟屏上的 App 被裁成兼容窗口（letterbox）

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 定义的**实现方案文档**（调查记录）。
> 范围：查清"锁定方向的 App 在我们的屏上被裁成一条、在同类工具的屏上铺满"的原因，并给出对策。
> 写完即冻结；后续结论另建文档。

**状态**：**已调查**（2026-10-07，机制查明 + 三轮实验 + 代码已复原）。

---

## 1. 现象

- 1920×1080 / 1920×1800 上跑 B 站：**App 只占中间一列**（46% / 77% 宽），两侧黑边、字号巨大、内容极少。
- 换 1440×3200（竖屏）：**铺满**。
- 同类工具 **AndroMeld**（同一台手机、同一个 App）：**拖动窗口大小都铺满**。

## 2. 查清的机制（设备侧实读）

1. **黑边在虚拟屏的原始帧缓冲里就有**（`screencap -d <SurfaceFlinger id>` 直接抓到）
   ⇒ 与 PC 渲染无关，PC 只是如实显示。
2. 系统的兼容窗口记录与配置：
   - `LetterboxDetails { letterboxInnerBounds = Rect(207,0 - 1018,990), letterboxFullBounds = Rect(0,0 - 1224,990) }`
   - 应用配置：`sw676dp w676dp h825dp 192dpi lrg **port**`（同参数下 AndroMeld 的屏是 `… xlrg **land**`）。
3. **窗口比例恒为 0.819 = 1/1.22，与屏尺寸、密度都无关**：

   | 屏 | 应用窗口 | 占宽 |
   |---|---|---|
   | 1920×1080 | 885×1080 | 46% |
   | 1920×1800 | 1475×1800 | 77% |
   | 1224×990 | 811×990 | 66% |

   而 `wm get-letterbox-style` 给出 **`Aspect ratio: 1.22`** —— 就是它。
   ⇒ **锁定方向（或声明 `SIZE_CHANGES_UNSUPPORTED`）的 App，系统只给"高÷宽 ≥ 1.22 的竖窗"**。
   B 站的特征：`targetSdk=32` · `supportsSizeChanges=SIZE_CHANGES_UNSUPPORTED_METADATA` ·
   `resizeMode=RESIZE_MODE_RESIZEABLE_VIA_SDK_VERSION`。
4. **由 3 直接推出的可用规律**：**屏的 高÷宽 ≥ 1.22** ⇒ 兼容窗口自然撑满。
   1440×3200（2.22）✓ 铺满；1920×1080（0.56）/ 1920×1800（0.94）/ 1224×990（0.81）✗ 必留边。

## 3. 同类工具（AndroMeld）的配方（设备侧实读 + APK 字符串）

| 项 | 实测 |
|---|---|
| 屏 | `AndroMeld Fusion` · 1224×990 @192dpi |
| flags | `FLAG_ROTATES_WITH_CONTENT` · `FLAG_PRESENTATION` · `FLAG_OWN_CONTENT_ONLY` · `FLAG_DESTROY_CONTENT_ON_REMOVAL` · `FLAG_TRUSTED` · `FLAG_ALWAYS_UNLOCKED` · `FLAG_TOUCH_FEEDBACK_DISABLED` · `FLAG_OWN_FOCUS` |
| 手段 | `Shizuku`×66 · `app_process`×7 · `VirtualDisplayConfig` + `DisplayManagerGlobal.createVirtualDisplay` · `ActivityOptions.setLaunchDisplayId` |
| 全局设置 | `settings put` / `force_resizable_activities` / `freeform` / `wm set` **0 命中** ⇒ 它不改系统设置 |
| 结果 | **同尺寸同密度**下 B 站**铺满**（`mBounds=Rect(0,0-1224,990)`，配置 `xlrg land`） |

## 4. 实验（每轮只改一个变量，实机验证）

| 轮 | 变量 | 结果 |
|---|---|---|
| 1 | 加 `FLAG_ROTATES_WITH_CONTENT` | **证伪** ✗ —— B 站仍被裁，`Rect(207,0-1018,990)` 与加之前逐字相同 |
| 2 | 加 `FLAG_PRESENTATION`（撤销轮 1） | **证伪** ✗ —— 同上（设备侧确认 flags 已变、结果不变） |
| 3 | 不请求 `FLAG_OWN_DISPLAY_GROUP`（其余保留） | **无法完成** ✗ —— 建屏回读 `期望 0xe88 实际 0xc88`（差 `0x200` = `FLAG_OWN_DISPLAY_GROUP`）⇒ **系统会随 `ALWAYS_UNLOCKED` 自动补上该位**，"不请求"做不到 |
| 4 | **把缺省解锁位改成 `false`**（连带不派生显示组） | ✅ **命中** —— 屏 flags 里**没有** `FLAG_OWN_DISPLAY_GROUP`，`ignoreOrientationRequest` **创建出来就是 `true`**，B 站配置 `sw825dp w1020dp h825dp 192dpi xlrg land`、窗口 `Rect(0,0-1224,990)` —— **与 AndroMeld 逐字相同** ✓ |

### 4.1 结论（第 4 轮实锤）

**因果链**：`ALWAYS_UNLOCKED` ⟶ 平台连带给出 `OWN_DISPLAY_GROUP` ⟶ 该屏的
`ignoreOrientationRequest` **恒为 `false` 且设不上去**（在第 3 轮的屏上实测：`wm set` 之后立刻
`get` 仍是 `false`）⟶ 锁定方向 App 的竖屏请求被认真对待 ⟶ 系统只能裁进 0.82 的兼容窗口。
**去掉解锁位 ⇒ 显示组消失 ⇒ 系统默认忽略方向请求 ⇒ App 铺满** ✓。

AndroMeld 的屏正是「**有 `ALWAYS_UNLOCKED`、但没有显示组**」：它用隐藏
`VirtualDisplayConfig` 建屏，绕过了这条连带关系 ⇒ 既保解锁又铺满。
**代价（`display-flags.md` §6.4 早已实测）**：关掉 `ALWAYS_UNLOCKED` 后，手机锁屏时虚拟屏会显示
keyguard（`mCurrentFocus` 变成 `com.android.systemui`，App 被挡住）；必要时可从 PC 注入解锁。

## 5. 结论与可落地的对策

1. **推荐（已验证）**：**不请求 `ALWAYS_UNLOCKED`**（也就不会有 `OWN_DISPLAY_GROUP`）⇒
   任意尺寸的屏上，锁定方向的 App 都按屏的整幅铺满（第 4 轮实测与 AndroMeld 逐字相同 ✓）。
   代价见 §4.1（keyguard 会挡住虚拟屏上的 App —— `display-flags.md` §6.4 已实测）。
2. **"两者兼得"在公开 API 下无解（2026-10-07 实测）**：换用公开的 `VirtualDisplayConfig` 建屏路径
   **同样**被平台强塞显示组（回读 `期望 0xe80 实际 0xc80`）⇒ 两条路都试过，已回滚。
   同类工具 AndroMeld 能同时拿到"解锁位 + 无显示组"，其手段**仍未查明**（见
   [`2026-10-07-screen-create-via-config.md`](2026-10-07-screen-create-via-config.md)）。
3. **尺寸选型的旧规则（仍然成立，可作备选）**：屏的 高÷宽 ≥ 1.22 ⇒ 兼容窗口自然撑满
   （1440×3200 ✓、1080×1920 ✓；1920×1800 ✗、1920×1080 ✗）。
4. **PC 侧**：`--dpi` 已落地（内容密度旋钮，见 [`2026-10-07-pc-dpi-flag.md`](2026-10-07-pc-dpi-flag.md)）；
   `--width/--height` 在采纳第 1 条后**不再受兼容窗口约束**，可按目标 App 的方向自由选。

## 6. 附带发现（另案，未修）

- **agent 重启期间的瞬时握手失败会让 PC 会话停摆**：日志 `重连失败：握手失败…（对端在握手完成前
  关闭了连接）` 之后再无重试，必须重启 PC 客户端才能恢复。
  **决定：把"握手失败"也纳入重连重试（带退避）—— 2026-10-07 用户已批准方向，但明确"先不修"，
  与本次 letterbox 调查并行的另一件事。**
- **`run-agent.sh` 在旧实例刚被 `kill -9` 后可能静默起不来**（观察两次）：`ps` 里没有 `wcreen`、
  logcat 无输出、端口探针"连上即被关闭" ⇒ 多半是新实例绑定抽象 socket 时撞上未释放的名字而退出。
  **对策（运维口径）**：`kill -9` 之后**等 1–2 秒**再启动，并**用 `ps -A | grep wcreen` 确认**已起来，
  不要只看脚本输出的 "已启动 pid=…"（那是 `setsid` 的返回值，不代表进程活着）。
- **多设备并存时 PC 的"转发自愈"会失效**（2026-10-07 实测）：`forward.rs` 调的是不带 `-s` 的
  `adb forward tcp:<port> localabstract:<name>`，而当同时接着手机与模拟器时，adb 直接报
  `adb.exe: more than one device/emulator` ⇒ 修复分支必然失败（诊断文本会照常给出，但结论是错的）。
  **未修**。将来接手的口径：要么给 PC 加 `--serial`（只影响 adb 调用，协议无关），
  要么从 `adb devices` 里按"唯一在线/已授权"自动选并**把选择打印出来**。
- `ignoreOrientationRequest` **不是**本问题的开关：虚拟屏默认即为 `true`（本机屏为 `false`），
  置位后 B 站照样被裁。

---

## 7. 变更记录

- **2026-10-07 调查 + 四轮实验**：机制查明（§2）、同类工具配方实测（§3）、四轮实验（§4，
  第 4 轮命中并复现 AndroMeld）、结论与对策（§5）、附带发现（§6）。
- **当前代码状态（2026-10-07 定稿）**：**用户已拍板采纳第 4 轮结论** ——
  `EncoderParams.DEFAULT_ALWAYS_UNLOCKED = false` + 能力表缺省同步为 `false`，实验注释已改为
  正式实现注释（含因果链与已实测的 keyguard 代价）；`display-flags.md` §3/§4、`display.md` 参数表
  与 §8 速览、`transport-shapes.md` 三处示例 JSON 已同步。
  **"两者兼得"（隐藏 `VirtualDisplayConfig` 建屏）记为备选路线，未实施。**
- 前三轮的实验改动（`ROTATES_WITH_CONTENT` / `PRESENTATION` / `OWN_DISPLAY_GROUP`）
  **已全部复原**，单测 62 项全绿。
