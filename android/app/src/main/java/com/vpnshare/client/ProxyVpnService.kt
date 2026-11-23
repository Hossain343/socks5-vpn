package com.vpnshare.client

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor

class ProxyVpnService : VpnService() {

    companion object {
        const val ACTION_START = "com.vpnshare.client.START"
        const val ACTION_STOP = "com.vpnshare.client.STOP"
        const val ACTION_TOGGLE = "com.vpnshare.client.TOGGLE"
        private const val CH = "vpn"
        private const val NID = 1
        // Must match DNS handling in the Rust core: queries to UDP/53 are answered locally.
        private const val TUN_ADDR = "10.8.0.2"
        private const val TUN_DNS = "10.8.0.1"
    }

    private val handler = Handler(Looper.getMainLooper())
    private var lastUp = 0L
    private var lastDown = 0L

    private val tick = object : Runnable {
        override fun run() {
            if (!VpnState.running) return
            val s = Native.stats()
            if (s.size < 5 || s[0] == 0L) {
                val e = Native.lastError()
                fail(if (e.isEmpty()) "اتصال قطع شد" else e)
                return
            }
            VpnState.upSpeed = s[1] - lastUp
            VpnState.downSpeed = s[2] - lastDown
            lastUp = s[1]; lastDown = s[2]
            VpnState.totalUp = s[1]; VpnState.totalDown = s[2]; VpnState.sessions = s[3]
            notifyState()
            handler.postDelayed(this, 1000)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Started via startForegroundService (widget / app): must call startForeground at once.
        goForeground("در حال آماده‌سازی…")
        when (intent?.action) {
            ACTION_STOP -> stopVpn()
            ACTION_TOGGLE -> if (VpnState.running) stopVpn() else startVpn()
            else -> startVpn() // ACTION_START, or the system's always-on VPN start
        }
        return START_NOT_STICKY
    }

    private fun startVpn() {
        if (VpnState.running) return
        VpnState.error = null
        VpnState.resetStats()
        val cfg = Prefs.load(this)
        if (!cfg.valid()) { fail("ابتدا آدرس و پورت را در تنظیمات وارد کنید"); openApp(); return }
        if (VpnService.prepare(this) != null) { fail("مجوز VPN داده نشده است"); openApp(); return }

        val b = Builder()
            .setSession("VPN")
            .setMtu(1500)
            .addAddress(TUN_ADDR, 24)
            .addDnsServer(TUN_DNS)
        try { b.addDisallowedApplication(packageName) } catch (_: Exception) {} // our own sockets never loop
        for ((ip, len) in Routes.compute()) b.addRoute(ip, len)
        b.addRoute(TUN_DNS, 32) // virtual DNS lives inside the excluded 10/8 range
        if (Build.VERSION.SDK_INT >= 29) b.setMetered(false)

        val pfd: ParcelFileDescriptor? = try { b.establish() } catch (_: Exception) { null }
        if (pfd == null) { fail("ساخت VPN ناموفق بود"); return }
        val fd = pfd.detachFd() // Rust owns and closes it

        if (!Native.start(fd, cfg.host.trim(), cfg.port, cfg.user, cfg.pass)) {
            try { ParcelFileDescriptor.adoptFd(fd).close() } catch (_: Exception) {}
            fail(Native.lastError().ifEmpty { "شروع هسته ناموفق بود" })
            return
        }
        lastUp = 0; lastDown = 0
        VpnState.running = true
        notifyState()
        handler.postDelayed(tick, 1000)
        ToggleWidget.refresh(this)
    }

    private fun fail(msg: String) {
        VpnState.error = msg
        stopVpn()
    }

    private fun stopVpn() {
        handler.removeCallbacks(tick)
        Native.stop()
        VpnState.running = false
        VpnState.upSpeed = 0; VpnState.downSpeed = 0
        ToggleWidget.refresh(this)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onRevoke() {
        stopVpn()
        super.onRevoke()
    }

    override fun onDestroy() {
        handler.removeCallbacks(tick)
        Native.stop()
        VpnState.running = false
        ToggleWidget.refresh(this)
        super.onDestroy()
    }

    private fun openApp() {
        try {
            startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (_: Exception) {}
    }

    private fun notifyState() {
        val text = "↓ ${Fmt.speed(VpnState.downSpeed)}   ↑ ${Fmt.speed(VpnState.upSpeed)}"
        getSystemService(NotificationManager::class.java).notify(NID, buildNotif(text))
    }

    private fun goForeground(text: String) {
        val n = buildNotif(text)
        if (Build.VERSION.SDK_INT >= 34) startForeground(NID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        else startForeground(NID, n)
    }

    private fun buildNotif(text: String): Notification {
        val nm = getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CH) == null) {
            nm.createNotificationChannel(NotificationChannel(CH, "VPN", NotificationManager.IMPORTANCE_LOW))
        }
        val pi = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        return Notification.Builder(this, CH)
            .setSmallIcon(R.drawable.ic_power)
            .setContentTitle("VPN متصل است")
            .setContentText(text)
            .setContentIntent(pi)
            .setOngoing(true)
            .build()
    }
}
