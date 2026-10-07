package com.xiaowine.wcreen.agent.transport

import java.io.FileDescriptor
import java.io.InputStream
import java.io.OutputStream

/**
 * 一条已建立的连接 —— 帧通路与控制通路都只认这个抽象，不认传输类型。
 *
 * `LocalSocket` 与 `Socket` 能力相同但没有公共父接口，所以在此声明一个最小接缝。
 */
internal interface Conn {

    /** 读侧 —— 只有 `reader` 会用它。 */
    val input: InputStream

    /** 写侧 —— 只有 `sender` 会用它。 */
    val output: OutputStream

    /**
     * 交给 `Os.poll` 做判活的 fd（`runtime-observability.md` §2.1）。
     *
     * **可能为 null**：`android.net.LocalSocket` 有 `getFileDescriptor()`，而
     * `java.net.Socket` **没有**（已用 SDK 的 `android.jar` 核对）。为 null 时判活
     * 退化为 [setReadTimeout] —— 判据相同（`T_dead` 内读不到任何字节即判死），
     * 只是机制不同。
     */
    val fd: FileDescriptor?

    /**
     * 设置读超时。
     *
     * 只在 [fd] 为 null 时被调用；有 fd 的传输实现成 no-op 即可。
     *
     * @param timeoutMs 超时毫秒数
     */
    fun setReadTimeout(timeoutMs: Int)

    /**
     * 只拆读侧：阻塞中的读立刻返回，**写侧仍可用**。
     *
     * 收尾时先做这一步，是为了让卡在读上的 `reader` 退出，同时还能让 `sender`
     * 把收尾消息发出去。
     */
    fun shutdownInput()

    /** 只拆写侧：对端看到 EOF。 */
    fun shutdownOutput()

    /** 关掉整条连接。幂等。 */
    fun close()
}

/**
 * 传输接缝 —— 唯一与 adb / 网络有关的地方（`runtime-threads.md` §5）。
 *
 * 连接方向固定：**agent 监听、PC 主动连**（两种传输一致）。
 */
internal interface Transport {

    /**
     * 阻塞等待一条新连接。
     *
     * @return 新连接；传输已被 [close] 时返回 null —— 那是正常退出路径，不是失败
     * @throws java.io.IOException 瞬时失败；调用方按「同一操作内重试 1 次」处理
     */
    fun accept(): Conn?

    /**
     * 叫醒正阻塞在 [accept] 里的线程。
     *
     * 必须有这一手：`LocalServerSocket.close()` **不会**让阻塞中的 `accept()` 返回
     * （实测：收尾要白等一个 join 超时）。所以收尾时先叫醒、再 join、最后才关监听。
     */
    fun wake()

    /** 关掉监听，让阻塞中的 [accept] 尽快返回。幂等。 */
    fun close()
}
