package com.xiaowine.wcreen.agent.transport

import android.net.LocalServerSocket
import android.net.LocalSocket
import android.net.LocalSocketAddress
import com.xiaowine.wcreen.agent.runtime.Ln
import java.io.FileDescriptor
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream

/**
 * adb 通道：设备侧 `LocalServerSocket(<抽象名>)`，PC 侧 `adb forward tcp:PORT localabstract:<名>`。
 *
 * 抽象名被占用 ⇒ 构造即抛 `IOException` ⇒ 上层判为「启动失败」（退出码 2）。
 * **重复实例就是靠这个天然判出来的**，不需要额外的实例检测步骤。
 *
 * @param name 抽象 socket 名
 * @param ln 日志出口
 * @throws IOException 绑定失败（名字被占用 / 权限不足）
 */
internal class AdbTransport(private val name: String, private val ln: Ln) : Transport {

    /** 监听 socket；`accept()` 与 `close()` 都经它。 */
    private val server: LocalServerSocket = LocalServerSocket(name)

    /** 是否已主动关闭 —— 用来把「关闭打断的 accept」与「真正的瞬时失败」分开。 */
    @Volatile
    private var closed: Boolean = false

    override fun accept(): Conn? = try {
        AdbConn(server.accept())
    } catch (e: IOException) {
        // close() 会让阻塞中的 accept 抛 IOException，这是正常退出路径。
        if (closed) null else throw e
    }

    override fun wake() {
        // 自己连自己一次：让阻塞中的 accept() 返回，循环随即看到停止标志而退出。
        val socket = LocalSocket()
        try {
            socket.connect(LocalSocketAddress(name))
        } catch (e: IOException) {
            ln.d("叫醒 accept 失败（$name）：${e.message}")
        } finally {
            try {
                socket.close()
            } catch (e: IOException) {
                ln.d("关闭叫醒用的 socket 失败（$name）：${e.message}")
            }
        }
    }

    override fun close() {
        closed = true
        try {
            server.close()
        } catch (e: IOException) {
            // 已经关掉的监听 socket 再 close 会抛，此时没有补救手段；
            // 打一条日志而不是静默吞掉。
            ln.d("adb 监听 socket 关闭时抛异常（$name）：${e.message}")
        }
    }
}

/**
 * `LocalSocket` 的 [Conn] 适配。
 *
 * @param socket 已接受的连接
 */
private class AdbConn(private val socket: LocalSocket) : Conn {

    override val input: InputStream get() = socket.inputStream

    override val output: OutputStream get() = socket.outputStream

    override val fd: FileDescriptor get() = socket.fileDescriptor

    /** adb 通道有 fd，判活走 `Os.poll`，这里不需要设读超时。 */
    override fun setReadTimeout(timeoutMs: Int) = Unit

    override fun shutdownInput() = socket.shutdownInput()

    override fun shutdownOutput() = socket.shutdownOutput()

    override fun close() = socket.close()
}
