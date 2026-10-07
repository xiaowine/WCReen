package com.xiaowine.wcreen.agent.transport

import com.xiaowine.wcreen.agent.runtime.Ln
import java.io.FileDescriptor
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.ServerSocket
import java.net.Socket

/**
 * 网络通道：设备侧 `ServerSocket(<端口>)`，PC 侧直连设备 IP。
 *
 * 端口被占用 ⇒ 构造即抛 `IOException` ⇒ 上层判为「启动失败」（退出码 2），
 * 与 adb 通道的重复实例检测同一条路。
 *
 * ⚠️ 裸 TCP **没有** adb 那样的密钥认证；网络侧认证是待定项，本实现不含认证。
 *
 * @param port 监听端口
 * @param ln 日志出口
 * @throws IOException 绑定失败（端口被占用 / 权限不足）
 */
internal class TcpTransport(private val port: Int, private val ln: Ln) : Transport {

    /** 监听 socket。 */
    private val server: ServerSocket = ServerSocket(port)

    /** 是否已主动关闭 —— 用来把「关闭打断的 accept」与「真正的瞬时失败」分开。 */
    @Volatile
    private var closed: Boolean = false

    override fun accept(): Conn? = try {
        TcpConn(server.accept())
    } catch (e: IOException) {
        if (closed) null else throw e
    }

    override fun wake() {
        // 自己连自己一次：让阻塞中的 accept() 返回，循环随即看到停止标志而退出。
        try {
            Socket("127.0.0.1", port).close()
        } catch (e: IOException) {
            ln.d("叫醒 accept 失败（端口 $port）：${e.message}")
        }
    }

    override fun close() {
        closed = true
        try {
            server.close()
        } catch (e: IOException) {
            ln.d("tcp 监听 socket 关闭时抛异常（端口 $port）：${e.message}")
        }
    }
}

/**
 * `Socket` 的 [Conn] 适配。
 *
 * @param socket 已接受的连接
 */
private class TcpConn(private val socket: Socket) : Conn {

    override val input: InputStream get() = socket.getInputStream()

    override val output: OutputStream get() = socket.getOutputStream()

    /**
     * 恒为 null —— `java.net.Socket` 没有 `getFileDescriptor()`，
     * 因此 TCP 的判活走 [setReadTimeout]。
     */
    override val fd: FileDescriptor? get() = null

    /** 把 `T_dead` 交给 socket 自身：超时读会抛 `SocketTimeoutException`（标准行为）。 */
    override fun setReadTimeout(timeoutMs: Int) {
        socket.soTimeout = timeoutMs
    }

    override fun shutdownInput() = socket.shutdownInput()

    override fun shutdownOutput() = socket.shutdownOutput()

    override fun close() = socket.close()
}
