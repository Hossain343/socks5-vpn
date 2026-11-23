package com.vpnshare.client

/** Full IPv4 space minus LAN / loopback / link-local / CGNAT / multicast, as CIDR routes. */
object Routes {
    private val excluded = listOf(
        "0.0.0.0/8", "10.0.0.0/8", "100.64.0.0/10", "127.0.0.0/8", "169.254.0.0/16",
        "172.16.0.0/12", "192.168.0.0/16", "224.0.0.0/4", "240.0.0.0/4"
    )

    private fun range(cidr: String): LongRange {
        val (ip, len) = cidr.split("/")
        val o = ip.split(".").map { it.toLong() }
        val start = (o[0] shl 24) or (o[1] shl 16) or (o[2] shl 8) or o[3]
        val size = 1L shl (32 - len.toInt())
        return start until (start + size)
    }

    fun compute(): List<Pair<String, Int>> {
        val ex = excluded.map { range(it) }.sortedBy { it.first }
        val free = mutableListOf<LongRange>()
        var cur = 0L
        for (r in ex) {
            if (r.first > cur) free += cur until r.first
            cur = maxOf(cur, r.last + 1)
        }
        if (cur <= 0xFFFFFFFFL) free += cur..0xFFFFFFFFL

        val out = mutableListOf<Pair<String, Int>>()
        for (r in free) {
            var s = r.first
            val e = r.last
            while (s <= e) {
                var size = if (s == 0L) 1L shl 32 else s and (-s)
                while (size > e - s + 1) size = size shr 1
                val prefix = 32 - java.lang.Long.numberOfTrailingZeros(size)
                out += "${(s shr 24) and 255}.${(s shr 16) and 255}.${(s shr 8) and 255}.${s and 255}" to prefix
                s += size
            }
        }
        return out
    }
}
