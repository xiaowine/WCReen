package com.xiaowine.wcreen.agent.runtime

import com.xiaowine.wcreen.agent.display.Screen
import com.xiaowine.wcreen.agent.encode.VideoEncoder
import com.xiaowine.wcreen.agent.input.InputQueue
import com.xiaowine.wcreen.agent.proto.Wire
import com.xiaowine.wcreen.agent.transport.Conn
import java.util.concurrent.ArrayBlockingQueue

/**
 * 一路 —— 一块屏 + 一条连接。
 *
 * 它持有本路的队列，创建并 join 自己的 `reader` / `link` / `sender`。
 *
 * @param id 路序号，从 1 开始；只用于日志
 * @param conn 本路的连接
 * @param ln 日志出口
 * @param shutdown 全局停止标志
 * @param coordinator 协调者
 * @param deps 进程级共享装配件（伪装 Context 造出的 `DisplayManager`、flag 常量、编码能力、主屏缺省）
 */
internal class Route(
    val id: Int,
    private val conn: Conn,
    private val ln: Ln,
    shutdown: Shutdown,
    private val coordinator: Coordinator,
    deps: RouteDeps,
) {

    /**
     * 控制消息队列。
     *
     * 容量 64 的依据：一条连接的控制消息只有心跳（每 3s 一条）与指令回报，
     * 队列只需覆盖写者短暂卡顿的窗口；写者真卡死由 `reader` 的 `T_dead`（10s）收口
     * ⇒ 64 条足够，且内存上界可界定。
     */
    private val controlQueue: ArrayBlockingQueue<Wire.Frame> =
        ArrayBlockingQueue(CONTROL_QUEUE_CAPACITY)

    /**
     * 帧队列。
     *
     * 容量 16 是**内存上限**（防炸），**不是丢弃判据** —— 判据是 `sender` 的等待时间 `T_drop`。
     * 60fps × 200ms = 12 帧 < 16 ⇒ 正常情况下 `sender` 会先丢，入队失败很少发生。
     */
    private val frameQueue: ArrayBlockingQueue<PendingFrame> =
        ArrayBlockingQueue(FRAME_QUEUE_CAPACITY)

    /** `sender` 的退出信号 —— 与全局停止标志分开，因为写者必须最后停。 */
    private val senderStop: Signal = Signal()

    /**
     * `link` 的退出信号。
     *
     * 连接断了就得让显示链路也停 —— 否则每条死连接都会留下一条永久空转的 `link` 线程。
     * 它释放屏与编码器（断开即拆屏；"断开后冻结、重连认领"是 S5 的语义）。
     */
    private val linkStop: Signal = Signal()

    /**
     * `input` 的退出信号。
     *
     * 它必须和连接一起停 —— 与 `link` 同理：否则每条死连接都会留下一条第 250 ms 空转一次
     * 的线程。收尾序列里它排在最前（`Teardown` 第 ③ 步）。
     */
    private val inputStop: Signal = Signal()

    /** 注入队列 —— 生产者是 `reader`（输入事件）与 `link`（目标屏变化）。 */
    private val inputQueue: InputQueue = InputQueue()

    /** 本路的屏。 */
    private val screen: Screen = Screen(ln, deps.virtualDevice, deps.flags)

    /** 本路的编码器。 */
    private val encoder: VideoEncoder = VideoEncoder(ln)

    /** 唯一读者。 */
    private val reader: ReaderLoop = ReaderLoop(
        routeId = id,
        conn = conn,
        ln = ln,
        shutdown = shutdown,
        coordinator = coordinator,
        caps = deps.caps,
        outbox = { offer(it) },
        linkCmd = { cmd -> link.post(cmd) },
        inputCmd = { cmd -> input.post(cmd) },
        onConnectionEnd = { endConnection() },
    )

    /**
     * 输入注入线程。
     *
     * 它在 `start()` 里起、在连接结束时随 `inputStop` 停 —— 与 `link` / `sender` 同规矩。
     */
    private val input: InputLoop = InputLoop(
        routeId = id,
        ln = ln,
        shutdown = shutdown,
        stopSignal = inputStop,
        coordinator = coordinator,
        injector = deps.inputInjector,
        queue = inputQueue,
        failAck = { requestId, fail -> reader.replyAck(requestId, false, fail, null) },
        reportEvent = { event, reason, subsystem -> reader.reportEvent(event, reason, subsystem) },
    )

    /** 显示链路（屏 + 编码器）。 */
    private val link: LinkLoop = LinkLoop(
        routeId = id,
        ln = ln,
        shutdown = shutdown,
        stopSignal = linkStop,
        screen = screen,
        encoder = encoder,
        caps = deps.caps,
        defaultWidth = deps.defaultWidth,
        defaultHeight = deps.defaultHeight,
        defaultDpi = deps.defaultDpi,
        frameQueue = frameQueue,
        ack = { requestId, ok, fail, applied -> reader.replyAck(requestId, ok, fail, applied) },
    )

    /** 唯一写者。 */
    private val sender: SenderLoop = SenderLoop(
        routeId = id,
        conn = conn,
        ln = ln,
        controlQueue = controlQueue,
        frameQueue = frameQueue,
        encoder = encoder,
        requestIdr = { link.post(LinkLoop.LinkCmd.RequestSync) },
        stopSignal = senderStop,
        coordinator = coordinator,
    )

    init {
        // link 的三条出口接回本路：致命走协调者、主动上报走协议、目标屏变化投给 input
        // —— 避免 link 反向依赖协调者、写者与输入线程。
        link.fatalSink = { fail -> coordinator.reportFatal(fail) }
        link.eventSink = { event, reason -> reader.reportEvent(event, reason) }
        link.inputSink = { cmd -> input.post(cmd) }
    }

    /**
     * 起本路的四条线程。
     *
     * 顺序：`reader` → `link` → `input` → `sender`（写者先于生产者）。队列在构造时就绪，
     * 所以先起谁都不会丢消息 —— `link` 发布的"目标屏"也一样能等到 `input` 起来再被取走。
     */
    fun start() {
        reader.start()
        link.start()
        input.start()
        sender.start()
        ln.i("路 #$id 已接入")
    }

    /**
     * 把一条消息投给本路的写者。
     *
     * @param frame 待发帧
     * @return false 表示队列已满（写者卡住）
     */
    fun offer(frame: Wire.Frame): Boolean = controlQueue.offer(frame)

    /**
     * 连接结束 —— 由 `reader` 退出时调用。
     *
     * 只拆读侧、不拆写侧：写者还要把队列里的收尾消息（例如 `stopAgent` 的回报）
     * 发出去，拆写侧会让它们全丢。
     */
    fun endConnection() {
        shutdownInput()
        linkStop.raise()
        // 输入也必须随连接停：否则每条死连接都留一条空转线程（`link` 当初就是栽在这里）。
        inputStop.raise()
        senderStop.raise()
        coordinator.onRouteFinished(this)
    }

    /** 拆读侧：让阻塞中的读立刻返回，写侧仍可用。 */
    fun shutdownInput() {
        try {
            conn.shutdownInput()
        } catch (e: Exception) {
            ln.d("路 #$id 拆读侧时抛异常：${e.message}")
        }
    }

    /** 拆写侧：对端看到 EOF。 */
    fun shutdownOutput() {
        try {
            conn.shutdownOutput()
        } catch (e: Exception) {
            ln.d("路 #$id 拆写侧时抛异常：${e.message}")
        }
    }

    /** 通知写者：排空后退出。 */
    fun stopSender() {
        senderStop.raise()
    }

    /**
     * 等读者结束。
     *
     * @param timeoutMs 超时毫秒数
     */
    fun joinReader(timeoutMs: Long) {
        reader.join(timeoutMs)
        if (reader.isAlive) ln.w("路 #$id 的 reader 在 ${timeoutMs} ms 内没退出，继续收尾")
    }

    /**
     * 等显示链路结束 —— 屏与编码器的释放发生在这条线程的 `finally` 里。
     *
     * @param timeoutMs 超时毫秒数
     */
    fun joinLink(timeoutMs: Long) {
        link.join(timeoutMs)
        if (link.isAlive) ln.w("路 #$id 的 link 在 ${timeoutMs} ms 内没退出，继续收尾")
    }

    /**
     * 通知输入线程退出。
     *
     * 队列里没处理的输入事件随之作废 —— 连接都没了，注入到哪里都没有意义。
     */
    fun stopInput() {
        inputStop.raise()
    }

    /**
     * 等输入线程结束。
     *
     * @param timeoutMs 超时毫秒数
     */
    fun joinInput(timeoutMs: Long) {
        input.join(timeoutMs)
        if (input.isAlive) ln.w("路 #$id 的 input 在 ${timeoutMs} ms 内没退出，继续收尾")
    }

    /**
     * 等写者结束。
     *
     * @param timeoutMs 超时毫秒数
     */
    fun joinSender(timeoutMs: Long) {
        sender.join(timeoutMs)
        if (sender.isAlive) ln.w("路 #$id 的 sender 在 ${timeoutMs} ms 内没退出，继续收尾")
    }

    /** 队列容量。 */
    companion object {

        /** 控制消息队列容量；依据见 [controlQueue]。 */
        private const val CONTROL_QUEUE_CAPACITY: Int = 64

        /** 帧队列容量（内存上限，不是丢弃判据）；依据见 [frameQueue]。 */
        private const val FRAME_QUEUE_CAPACITY: Int = 16
    }
}
