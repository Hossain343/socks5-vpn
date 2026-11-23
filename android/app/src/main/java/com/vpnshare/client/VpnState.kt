package com.vpnshare.client

/** Process-wide live state, written by the service, read by the UI and widget. */
object VpnState {
    @Volatile var running = false
    @Volatile var error: String? = null
    @Volatile var upSpeed = 0L
    @Volatile var downSpeed = 0L
    @Volatile var totalUp = 0L
    @Volatile var totalDown = 0L
    @Volatile var sessions = 0L

    fun resetStats() { upSpeed = 0; downSpeed = 0; totalUp = 0; totalDown = 0; sessions = 0 }
}
