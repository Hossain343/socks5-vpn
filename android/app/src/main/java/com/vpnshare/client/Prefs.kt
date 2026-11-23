package com.vpnshare.client

import android.content.Context

/** Host/port are written ONLY by [save], which only the Settings "Apply" button calls. */
object Prefs {
    private const val FILE = "vpn_config"

    data class Cfg(val host: String, val port: Int, val user: String, val pass: String) {
        fun valid() = host.isNotBlank() && port in 1..65535
    }

    fun load(c: Context): Cfg {
        val p = c.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        return Cfg(
            p.getString("host", "") ?: "",
            p.getInt("port", 1080),
            p.getString("user", "") ?: "",
            p.getString("pass", "") ?: ""
        )
    }

    fun save(c: Context, cfg: Cfg) {
        c.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit()
            .putString("host", cfg.host.trim())
            .putInt("port", cfg.port)
            .putString("user", cfg.user)
            .putString("pass", cfg.pass)
            .commit() // synchronous: survives an immediate process kill
    }
}
