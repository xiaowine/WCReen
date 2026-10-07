#!/system/bin/sh
#
# WCReen agent 启动脚本（设备端）。
#
# 用法：
#   run-agent.sh adb --name=wcreen
#   run-agent.sh tcp --port=27183
#
# 本脚本只负责**启动**。控制通道由协议承担（8 条指令），日志走 logcat —— 不落盘。
# 启动阶段的失败在这一层看不到（进程被后台化了），只能靠 logcat 定位；
# 这是已知缺口：`app_process` 参数解析 / 类加载阶段还没有 logcat 可用。
#
# 参数含义见 docs/spec/agent/runtime-startup.md §1.1。

set -u

JAR="/data/local/tmp/wcreen-agent.jar"
CLASS="com.xiaowine.wcreen.agent.Main"

if [ ! -f "$JAR" ]; then
    echo "找不到 $JAR（先 adb push 产物过去）" >&2
    exit 2
fi

if [ $# -lt 1 ]; then
    echo "用法：$0 <传输: adb|tcp> [选项]" >&2
    exit 2
fi

export CLASSPATH="$JAR"

# 必须脱离 adb 连接。不脱离时 adb 一断 agent 就死，系统会连带回收虚拟屏，
# 屏上 App 的状态全丢 —— "断开后冻结、重连即解冻"这个模型就不成立了。
#
# 三段都不能省：
#   nohup                       忽略 SIGHUP
#   setsid                      另起会话（toybox 的 setsid 在调用者已是进程组长时会
#                               静默失败，所以不能只靠它，必须配合后台化）
#   </dev/null >/dev/null 2>&1  脱离后没有终端，标准流一律丢弃
#
# --nice-name 必须放在 cmd-dir 分隔符 "/" 之后 —— 放错会静默失败：
# 进程起来、退出码 0、零输出。
nohup setsid app_process /system/bin --nice-name=wcreen "$CLASS" "$@" \
    </dev/null >/dev/null 2>&1 &

echo "已启动 pid=$!（日志：adb logcat -s WCReen）"
