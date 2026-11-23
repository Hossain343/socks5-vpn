package com.vpnshare.client

object Fmt {
    fun bytes(b: Long): String = when {
        b >= 1L shl 30 -> String.format("%.2f GB", b / 1073741824.0)
        b >= 1L shl 20 -> String.format("%.1f MB", b / 1048576.0)
        b >= 1L shl 10 -> String.format("%.0f KB", b / 1024.0)
        else -> "$b B"
    }
    fun speed(b: Long) = bytes(b) + "/s"
}
