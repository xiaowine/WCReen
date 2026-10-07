# `:agent` · 拉起应用（F2）实机验证结果

> 本文件是 [`agent/launch.md`](launch.md) 的**实机验证结果**，从该文件拆出（原文件超单文件上限）。
> 规范正文见 [`agent/launch.md`](launch.md)；枚举细节见 [`agent/launch-enumerate.md`](launch-enumerate.md)。

**已全部验证完毕**（本机 Redmi 22127RK46C / Android 16 / SDK 36，一次性探针，测完已删）。

| # | 原未知 | 实测结论 |
|---|---|---|
| V1 | 第三方 App 能否真的在虚拟屏上跑起来 | **能**。普通第三方 uid（如 `com.absinthe.libchecker`，A=10370）在虚拟屏上 `RESUMED`，且**输出面帧计数递增**（162 → 256，+94 帧）⇒ 不只是"task 挂上了"，是**真的在渲染** |
| V2 | 同一 App 能否同时出现在两块屏 | **默认不能**（task 被搬走，同一个 `ActivityRecord`，状态随 task 保留）；加 `--activity-multiple-task` 可以，**实测至少 8 份**且**共用同一进程**。不同 App 天然可以。**已决定：PC 侧限单实例 ⇒ 一律不加该 flag** ⇒ 详见 [`agent/launch.md`](launch.md) §4.2.1 |
| V3 | 屏被释放时其上 task 的命运 | **由 `DESTROY_CONTENT_ON_REMOVAL` 决定，与释放路径无关**：开 ⇒ 内容留成不可见僵尸 task、**本机屏干净**；关 ⇒ **内容搬到本机屏并 `visible=true`**。**显式 `release()` 与进程崩溃（`halt`）行为完全一致**（该 flag 由 system_server 侧生效，不依赖我们的进程活着） |
| V4 | `app_process` 下能否取别的包的 label | **能**。`ApplicationPackageManager.getApplicationInfo().loadLabel()` 返回本地化文本（设置 / 浏览器 / 夸克 / 支付宝），**禁用包也能取**。同时实测 `nonLocalizedLabel` **全为 null** ⇒ 印证 [`launch-enumerate.md`](launch-enumerate.md) §2「label 必须走进程内」 |
| V5 | 进程内取"某屏顶层 activity"的隐藏 API | **有**：`IActivityTaskManager.getAllRootTaskInfosOnDisplay(int)`，实测调用成功并返回结构化数据 ⇒ 不需要解析 dumpsys。详见 [`agent/launch.md`](launch.md) §4.3 |
| V6 | 系统 / 禁用 / 挂起 App 的表现 | 禁用包**不进**默认名单；强行发起 ⇒ `Error type 3`、退出码 1、**无副作用**。详见 [`launch-enumerate.md`](launch-enumerate.md) §4.1 与 §5.1 |

**附带发现（同样已实测）**：

1. **入口转发是常态**：两个不同 App（设置、GitHub）都把请求的入口转发到另一个 Activity ⇒ 判据只能到**包名**级别（见 [`agent/launch.md`](launch.md) §4.3）。
2. **切换 App 不销毁旧 task**：往同一块屏拉第二个 App，旧 task 仍留在该屏（`visible=false`），不销毁。
3. **`dumpsys activity activities` 的段顺序不稳定**（聚焦屏排最前），解析必须按段头切分。
4. **失败后无残留**：三种失败都不产生 task、不改目标屏顶层。

---

## 变更记录

- **2026-10-06 拆分**：本文件由 [`agent/launch.md`](launch.md) §7 整段提成独立文件
  （原文件超单文件上限），内容未改。
