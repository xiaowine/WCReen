# `:agent` 隐藏 API 的访问形态

> 本文件是 [`docs/spec/workflow.md`](../spec/workflow.md) §2 定义的**实现方案文档**。
> 范围：**怎么访问隐藏 API** —— 取法、编译期检查、运行期兜底、以及工程接入方式。
> 写完即冻结；实施中的偏差另建文档。

**状态**：**待批准**（2026-10-07）。

---

## 1. 背景与目标

`display.md` §2.2 的 Context 引导、`input.md` 的注入、`launch.md` §4.3 的判据都依赖隐藏 API。
现行规范只说「反射集中在 `wrappers/`」，**反射在编译期无法静态发现问题** ——
名字/签名打错、成员漂移都要等到运行期才炸，且**炸在哪个调用点取决于用到哪个功能**。

**目标**：把能变成**编译期可检查**的访问改成直接调用；剩下无法避免的反射**集中 + 前移失败**。

**验收标准**：① 直接调用点在编译期即可发现签名错误；② 残余反射点**全部在启动时一次性核验**，
缺失即 fatal 并列出缺哪条；③ **不改动机器上的 SDK**。

---

## 2. 现状：隐藏 API 的真实清单（6 处）

| # | 成员 | 出处 |
|---|---|---|
| 1 | `Looper.sMainLooper`（**private** 字段） | [`agent/display.md`](../spec/agent/display.md) §2.2 引导链 |
| 2 | `ActivityThread` 构造器 + `sCurrentActivityThread` / `mSystemThread` / `mConfigurationController` / `mBoundApplication` / `mInitialApplication` / `getSystemContext()` | 同上 |
| 3 | `DisplayManager(Context)` | 同上第 2 条 |
| 4 | `DisplayInfo` 的色彩字段 | [`agent/encode-capability.md`](../spec/agent/encode-capability.md) |
| 5 | `ActivityTaskManager.getService()` → `IActivityTaskManager.getAllRootTaskInfosOnDisplay(int)` | [`agent/launch.md`](../spec/agent/launch.md) §4.3 |
| 6 | `IInputManager.injectInputEvent` | [`agent/input.md`](../spec/agent/input.md) |

**不需要隐藏 API 的**（已确认）：建屏/换面走**公开** `DisplayManager.createVirtualDisplay` 与
`VirtualDisplay.setSurface()`；编码器能力探测**完全公开**（`encode-probe.md` §11）。

---

## 3. 本次实测（决定方案的四条）

> 环境：设备 `7766c4b3`（Android 16 / SDK 36 / uid 2000）；本机 `platforms/android-37.0`；
> 自定义 jar = [`Reginer/aosp-android-jar`](https://github.com/Reginer/aosp-android-jar) 的
> `android-37/android.jar`（**AOSP `android-17.0.0_r1` 原样编译**，58,780,395 B，MIT）。
> 实验工程在仓库外的临时目录，`:agent` 的构建配置逐字复刻（AGP 9.3.3 / `compileSdk 37` / JDK 21）。

| # | 结论 | 证据 |
|---|---|---|
| **1** | **直接调用隐藏 API 在本进程不被拦** ⇒ 不必引入 `AndroidHiddenApiBypass` | `app_process` 探针：反射构造 `ActivityThread` ✓ **且**直接调用 `ServiceManager.getService("display")` ✓（编译期经手写桩、桩不进 dex） |
| **2** | **`compileOnly` 会被 SDK 的 `android.jar` 遮蔽，且是「类级」遮蔽** | `Unresolved reference 'sMainLooper'`；`Unresolved reference 'setMainLooperForTest'`；`Cannot access 'constructor(): DisplayManager'` |
| **3** | **配置层无解** —— SDK 的 `android.jar` 由 AGP **单独注入**，不在任何 dependency configuration 里 | `releaseCompileClasspath` = `[自定义 jar, kotlin-stdlib, annotations]`，**无 android.jar**；`androidApis` 在 AGP 9.3.3 **不存在** |
| **4** | **SDK 里「不存在」的类，`compileOnly` 完全可用**，且**不打包** | `BUILD SUCCESSFUL`；`assembleRelease` 产物 **2,085,034 B**、`classes.dex` 单 dex、**零 `android/*` 打包** |

### 3.1 成员可见性实况（javap 自定义 jar）

| 成员 | 实况 | 后果 |
|---|---|---|
| `DisplayInfo.colorMode` / `supportedColorModes` | **`public` 字段**（不是 `getColorMode()` 方法） | 可直接读 ✓ |
| `IInputManager.injectInputEvent(InputEvent, int)` | **2 参**（另有 `injectInputEventToTarget(InputEvent,int,int)`） | 可直接调 ✓ |
| `ActivityThread.currentActivityThread()` / `getSystemContext()` | `public` | 可直接调 ✓ |
| `ActivityThread.mSystemThread` / `mBoundApplication` / `mInitialApplication` | **package-private** | **语言层不许访问 ⇒ 只能反射** ✗ |
| `ActivityThread.sCurrentActivityThread` / `mConfigurationController` | **private**（jar 里没有） | 只能反射 ✗ |
| `Looper.sMainLooper` | **private**（jar 里没有） | 只能反射 ✗ |

---

## 4. 方案与取舍

### 4.1 采纳：**`compileOnly` 自定义 jar + 能直接调的直调 + 残余反射 + 启动自检**

| # | 点 | 形态 |
|---|---|---|
| 1 | `Looper.sMainLooper` | **反射**（private，无可替代） |
| 2a | `ActivityThread` 类 / `currentActivityThread()` / `getSystemContext()` | **直接调用**（编译期全查） |
| 2b | `ActivityThread` 的 5 个非 public 字段 | **反射**（private / package-private） |
| 3 | `DisplayManager(Context)` | **反射**（类在 SDK 里 ⇒ 被遮蔽；替换 jar 可解，但不划算，见 4.2） |
| 4 | `DisplayInfo.colorMode` | **直接读** |
| 5 | `ActivityTaskManager.getService()` + `IActivityTaskManager` | **直接调用** |
| 6 | `IInputManager.injectInputEvent`（绕开 SDK 里的 `InputManager`） | **直接调用** |

**净收益**：**6 处 → 4 处直接调用 + 3 组反射**，且 3 组反射**全部集中在启动引导**一次完成 ⇒
正好落在启动自检能覆盖的范围内。

### 4.2 被否方案

| 方案 | 否掉的理由 |
|---|---|
| **替换 `${SDK}/platforms/android-37.0/android.jar`**（两个上游项目的推荐做法） | 是**机器级改动**（影响本机所有项目），且**只换来 1 处**（点 3）—— 收益与代价不匹配 |
| **影子 SDK 根**（`local.properties` 的 `sdk.dir` 指向带自定义 jar 的 SDK 副本） | 同上：只换来 1 处，却要多一层目录联结与设置脚本 |
| **用 `anggrayudi/android-hidden-api` 的 `hiddenjar` 自己生成** | 它的产物**含 private 成员**，但 **private 成员语言层就不许访问** ⇒ 对点 1/2b **毫无帮助** ⇒ 白付一整条工具链（CLI + `dex-tools` + AVD 启动） |
| **`AndroidHiddenApiBypass`** | **实测本进程不拦**（§3 #1）⇒ 无用。**记为退路**：若将来某 ROM 开始拦，它就是把 `setHiddenApiExemptions("")` 打上的现成手段 |
| **`hiddenjar` 的完整覆盖（`$BOOTCLASSPATH` 52 个 jar）** | 我们的 6 个点全在 `framework.jar` 里 ⇒ 用不上 mainline/APEX 的覆盖面 |

---

## 5. 影响面

| 区域 | 变化 |
|---|---|
| `agent/libs/android-37-hidden.jar` | **新增**（58.8 MB，**不入库**；由获取脚本下载 + 校验） |
| `agent/build.gradle.kts` | 加一行 `compileOnly(files("libs/android-37-hidden.jar"))` |
| `agent/src/main/kotlin/.../wrappers/` | 新增隐藏 API 封装（含残余反射点） |
| `agent/src/main/kotlin/.../runtime/` | 新增**启动自检**（在 `runtime-startup.md` 的六步顺序中插入） |
| [`agent/code.md`](../spec/agent/code.md) §10 | **改写**：从「唯一允许**反射**的包」→「唯一允许出现**隐藏 API** 的包；**优先直调**，残余反射须登记并进自检」 |
| [`agent/module.md`](../spec/agent/module.md) §4 / §8 | 补：隐藏 API jar 的获取方式；禁忌「裸反射」 |
| `agent/scripts/` | 新增 jar 获取/校验脚本（该目录目前**不存在**） |
| `docs/code-map.md` | 同步 |

**对外契约**：**不变**（线协议 / CLI / 部署路径均不动）。

---

## 6. 实施步骤

1. **获取 jar**：脚本从 `Reginer/aosp-android-jar` 的 `android-37/android.jar` 下载到 `agent/libs/`，
   **校验字节数 58780395**（版本/来源可追溯），并加进 `.gitignore`；
2. **接进构建**：`agent/build.gradle.kts` 加 `compileOnly`；**不替换 SDK 的任何文件**；
3. **写 `wrappers/`**：点 2a/4/5/6 走**直接调用**；点 1/2b/3 走**反射**，成员名与签名**只在一处出现**；
4. **启动自检**：启动时一次性核验 **7 项**（4 个直接调用点 + 3 个反射点）；
   任一不符 ⇒ **启动失败**（退出码 `2`）并列出缺哪条；
5. **回填文档**：`code.md` §10、`module.md` §4/§8、`code-map.md`。

---

## 7. 验证方式

| 层 | 手段 |
|---|---|
| 编译期 | 故意把某个直调点的签名写错 ⇒ **编译必须失败**（证明编译期真的查得到） |
| 构建产物 | `assembleRelease` ⇒ 产物大小量级 **~2 MB**、单 `classes.dex`、**无 `android/*`** |
| 运行期 | 实机起进程 ⇒ 自检 7 项全过；再把某一项人为破坏（如换设备/改桩名）⇒ **启动失败并列出缺哪条** |
| 功能 | 按 `display.md` / `input.md` / `launch.md` 各自既有的实机验收判据复跑 |

---

## 8. 风险与回滚

| 风险 | 应对 |
|---|---|
| 自定义 jar 与**运行设备**不一致（jar 是 **37**，设备是 **36**） | **启动自检**覆盖；且 6 个成员从 API 1 就存在，风险低 |
| jar 来源是第三方二进制（58.8 MB） | 校验字节数 + 记录来源与 AOSP 分支（`android-17.0.0_r1`）；换来源时重跑验证 |
| `compileOnly` jar 意外进产物 | 已验证不打包；纳入构建产物检查项 |
| 某 ROM 开始拦隐藏 API | `AndroidHiddenApiBypass` 退路（§4.2） |
| 回滚 | 删 `compileOnly` 一行 + 恢复 `wrappers/` 为纯反射形态 ⇒ 无残留 |

---

## 9. 未决问题

| # | 问题 |
|---|---|
| 1 | `Looper` 是否可用**公开**的 `setMainLooperForTest(Looper)` 替代写 `sMainLooper`？—— 它在 SDK 里被遮蔽，需**替换 jar** 才能验证；**待定** |
| 2 | 自定义 jar 的**更新策略**（跟随 `compileSdk` 升级 / 换设备重取）—— 待定 |
| 3 | `InputManager` 与 `IInputManager` 走哪条（现方案选 `IInputManager`，因为它在 SDK 里不存在）—— 与 `input.md` 的取法需对齐 |

---

## 10. 变更记录

- **2026-10-07 起草**：定下隐藏 API 的访问形态。**四条实测**：① 本进程**不拦**隐藏 API（直接调用可行，
  `AndroidHiddenApiBypass` 不需要）；② `compileOnly` 被 SDK 的 `android.jar` **类级遮蔽**；
  ③ SDK 的 `android.jar` **不在任何 dependency configuration 里**（AGP 单独注入）⇒ 配置层无解；
  ④ **SDK 里不存在的类，`compileOnly` 可用且不打包**。
  **结论**：**不动机器上的 SDK**，用 `compileOnly` + 直调（4 处）+ 残余反射（3 组，全在启动引导）+
  **启动自检**；`6 处 → 4 直调 + 3 反射`。被否：替换 SDK jar / 影子 SDK（只为 1 处，代价是机器级改动）、
  `hiddenjar` 自建（private 成员照样不能直调）、`AndroidHiddenApiBypass`（实测无用，记为退路）。
