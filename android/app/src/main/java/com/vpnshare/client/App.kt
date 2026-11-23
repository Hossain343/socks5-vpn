package com.vpnshare.client

import android.app.Application

class App : Application() {
    override fun onCreate() {
        super.onCreate()
        // A fresh process means no VPN is running: repaint any stale red widget as grey.
        ToggleWidget.refresh(this)
    }
}
