package com.vpnshare.client

object Native {
    init { System.loadLibrary("vpnclient") }

    /** Takes ownership of [fd] (a detached TUN fd). Returns false on failure; then the caller must close the fd. */
    @JvmStatic external fun start(fd: Int, host: String, port: Int, user: String, pass: String): Boolean
    @JvmStatic external fun stop()

    /** [connected(0/1), bytesUp, bytesDown, activeSessions, errors] */
    @JvmStatic external fun stats(): LongArray
    @JvmStatic external fun lastError(): String
}
