package com.vpnshare.client

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews

class ToggleWidget : AppWidgetProvider() {

    override fun onUpdate(context: Context, mgr: AppWidgetManager, ids: IntArray) {
        // Always rebuilt from live state: a stale "connected" never survives a process restart.
        for (id in ids) mgr.updateAppWidget(id, build(context))
    }

    companion object {
        fun build(ctx: Context): RemoteViews {
            val on = VpnState.running
            val rv = RemoteViews(ctx.packageName, R.layout.widget_toggle)
            rv.setInt(R.id.widget_root, "setBackgroundResource", if (on) R.drawable.widget_bg_on else R.drawable.widget_bg_off)
            rv.setTextViewText(R.id.widget_label, if (on) "متصل" else "قطع")
            val i = Intent(ctx, ProxyVpnService::class.java).setAction(ProxyVpnService.ACTION_TOGGLE)
            val pi = PendingIntent.getForegroundService(
                ctx, 0, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
            rv.setOnClickPendingIntent(R.id.widget_root, pi)
            return rv
        }

        fun refresh(ctx: Context) {
            val c = ctx.applicationContext
            val mgr = AppWidgetManager.getInstance(c)
            val ids = mgr.getAppWidgetIds(ComponentName(c, ToggleWidget::class.java))
            if (ids.isNotEmpty()) mgr.updateAppWidget(ids, build(c))
        }
    }
}
