package com.vpnshare.client

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.graphics.Color
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import android.widget.TextView
import android.widget.Toast
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import com.google.android.material.bottomnavigation.BottomNavigationView
import com.google.android.material.button.MaterialButton
import com.google.android.material.textfield.TextInputEditText

class MainActivity : AppCompatActivity() {

    private val h = Handler(Looper.getMainLooper())
    private val ticker = object : Runnable {
        override fun run() { render(); h.postDelayed(this, 1000) }
    }
    private val RED = Color.parseColor("#E53935")
    private val GREY = Color.parseColor("#757575")

    private lateinit var pageMain: View
    private lateinit var pageSettings: View
    private lateinit var btn: FrameLayout
    private lateinit var status: TextView
    private lateinit var errorTv: TextView
    private lateinit var speedTv: TextView
    private lateinit var totalTv: TextView
    private lateinit var etHost: TextInputEditText
    private lateinit var etPort: TextInputEditText
    private lateinit var etUser: TextInputEditText
    private lateinit var etPass: TextInputEditText

    private val vpnPermission = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        if (it.resultCode == RESULT_OK) startService(ProxyVpnService.ACTION_START)
    }
    private val notifPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        pageMain = findViewById(R.id.page_main)
        pageSettings = findViewById(R.id.page_settings)
        btn = findViewById(R.id.btn_power)
        status = findViewById(R.id.tv_status)
        errorTv = findViewById(R.id.tv_error)
        speedTv = findViewById(R.id.tv_speed)
        totalTv = findViewById(R.id.tv_total)
        etHost = findViewById(R.id.et_host)
        etPort = findViewById(R.id.et_port)
        etUser = findViewById(R.id.et_user)
        etPass = findViewById(R.id.et_pass)

        val cfg = Prefs.load(this)
        etHost.setText(cfg.host)
        etPort.setText(cfg.port.toString())
        etUser.setText(cfg.user)
        etPass.setText(cfg.pass)

        findViewById<BottomNavigationView>(R.id.nav).setOnItemSelectedListener {
            showSettings(it.itemId == R.id.nav_settings); true
        }
        btn.setOnClickListener { toggle() }
        findViewById<MaterialButton>(R.id.btn_apply).setOnClickListener { apply() }

        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) notifPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
    }

    override fun onResume() { super.onResume(); h.post(ticker) }
    override fun onPause() { h.removeCallbacks(ticker); super.onPause() }

    private fun showSettings(s: Boolean) {
        pageSettings.visibility = if (s) View.VISIBLE else View.GONE
        pageMain.visibility = if (s) View.GONE else View.VISIBLE
    }

    private fun startService(action: String) {
        ContextCompat.startForegroundService(this, Intent(this, ProxyVpnService::class.java).setAction(action))
    }

    private fun toggle() {
        if (VpnState.running) { startService(ProxyVpnService.ACTION_STOP); return }
        if (!Prefs.load(this).valid()) {
            Toast.makeText(this, "ابتدا در تنظیمات آدرس و پورت را وارد کنید", Toast.LENGTH_LONG).show()
            findViewById<BottomNavigationView>(R.id.nav).selectedItemId = R.id.nav_settings
            return
        }
        val i = VpnService.prepare(this)
        if (i != null) vpnPermission.launch(i) else startService(ProxyVpnService.ACTION_START)
    }

    private fun apply() {
        val host = etHost.text?.toString()?.trim().orEmpty()
        val port = etPort.text?.toString()?.trim()?.toIntOrNull() ?: -1
        if (host.isEmpty() || port !in 1..65535) {
            Toast.makeText(this, "آدرس یا پورت نامعتبر است", Toast.LENGTH_LONG).show()
            return
        }
        Prefs.save(this, Prefs.Cfg(host, port, etUser.text?.toString().orEmpty(), etPass.text?.toString().orEmpty()))
        (getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(etHost.windowToken, 0)
        Toast.makeText(
            this,
            if (VpnState.running) "ذخیره شد. برای اعمال، یک بار قطع و وصل کنید" else "ذخیره شد",
            Toast.LENGTH_LONG
        ).show()
    }

    private fun render() {
        val on = VpnState.running
        btn.backgroundTintList = ColorStateList.valueOf(if (on) RED else GREY)
        status.text = if (on) "متصل" else "قطع"
        val err = VpnState.error
        errorTv.visibility = if (!on && err != null) View.VISIBLE else View.GONE
        errorTv.text = err ?: ""
        speedTv.visibility = if (on) View.VISIBLE else View.INVISIBLE
        totalTv.visibility = speedTv.visibility
        speedTv.text = "↓ ${Fmt.speed(VpnState.downSpeed)}     ↑ ${Fmt.speed(VpnState.upSpeed)}"
        totalTv.text = "مجموع: ↓ ${Fmt.bytes(VpnState.totalDown)}   ↑ ${Fmt.bytes(VpnState.totalUp)}"
    }
}
