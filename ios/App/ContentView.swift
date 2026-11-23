import SwiftUI
import NetworkExtension

private func fmtBytes(_ b: Int64) -> String {
    let d = Double(b)
    if d >= 1_073_741_824 { return String(format: "%.2f GB", d / 1_073_741_824) }
    if d >= 1_048_576 { return String(format: "%.1f MB", d / 1_048_576) }
    if d >= 1024 { return String(format: "%.0f KB", d / 1024) }
    return "\(b) B"
}

struct ContentView: View {
    @StateObject private var vpn = VpnManager()
    @State private var tab = 0

    var body: some View {
        TabView(selection: $tab) {
            MainTab(vpn: vpn, goSettings: { tab = 1 })
                .tabItem { Label("اتصال", systemImage: "power") }
                .tag(0)
            SettingsTab(vpn: vpn)
                .tabItem { Label("تنظیمات", systemImage: "gearshape") }
                .tag(1)
        }
    }
}

struct MainTab: View {
    @ObservedObject var vpn: VpnManager
    let goSettings: () -> Void

    private var on: Bool { vpn.status == .connected }
    private var statusText: String {
        switch vpn.status {
        case .connected: return "متصل"
        case .connecting, .reasserting: return "در حال اتصال…"
        case .disconnecting: return "در حال قطع…"
        default: return "قطع"
        }
    }

    var body: some View {
        VStack(spacing: 20) {
            Spacer()
            Button(action: {
                if vpn.host.isEmpty && !on { goSettings() } else { vpn.toggle() }
            }) {
                ZStack {
                    Circle().fill(on ? Color.red : Color.gray).frame(width: 220, height: 220).shadow(radius: 12)
                    Image(systemName: "power").font(.system(size: 90, weight: .bold)).foregroundColor(.white)
                }
            }
            .buttonStyle(PlainButtonStyle())
            Text(statusText).font(.system(size: 32, weight: .bold)).padding(.top, 12)
            if let e = vpn.error, !on {
                Text(e).foregroundColor(.red).multilineTextAlignment(.center).padding(.horizontal)
            }
            if on {
                Text("↓ \(fmtBytes(vpn.downSpeed))/s     ↑ \(fmtBytes(vpn.upSpeed))/s").font(.system(size: 18))
                Text("مجموع: ↓ \(fmtBytes(vpn.totalDown))   ↑ \(fmtBytes(vpn.totalUp))")
                    .font(.system(size: 14)).opacity(0.7)
            }
            Spacer()
        }
        .padding()
    }
}

struct SettingsTab: View {
    @ObservedObject var vpn: VpnManager
    @State private var host = ""
    @State private var port = ""
    @State private var user = ""
    @State private var pass = ""
    @State private var message: String?
    @State private var filled = false

    var body: some View {
        Form {
            Section(header: Text("تنظیمات سرور")) {
                TextField("آدرس سرور (IP یا دامنه)", text: $host)
                    .keyboardType(.URL).autocapitalization(.none).disableAutocorrection(true)
                TextField("پورت", text: $port).keyboardType(.numberPad)
                TextField("نام کاربری (اختیاری)", text: $user)
                    .autocapitalization(.none).disableAutocorrection(true)
                SecureField("رمز عبور (اختیاری)", text: $pass)
            }
            Section {
                Button("اعمال و ذخیره") { save() }
                if let m = message { Text(m).font(.footnote) }
                Text("این مقادیر فقط با زدن «اعمال و ذخیره» تغییر می‌کنند.").font(.footnote).opacity(0.7)
            }
        }
        .onReceive(vpn.$loaded) { l in
            // Fill the form once from the saved configuration; never overwrite what the user is typing.
            if l && !filled {
                host = vpn.host; port = String(vpn.port); user = vpn.user; pass = vpn.pass
                filled = true
            }
        }
    }

    private func save() {
        let h = host.trimmingCharacters(in: .whitespaces)
        guard !h.isEmpty, let p = Int(port.trimmingCharacters(in: .whitespaces)), (1...65535).contains(p) else {
            message = "آدرس یا پورت نامعتبر است"
            return
        }
        let running = vpn.status == .connected
        vpn.apply(host: h, port: p, user: user, pass: pass) { err in
            if let err = err { message = err }
            else { message = running ? "ذخیره شد. برای اعمال، یک بار قطع و وصل کنید" : "ذخیره شد" }
        }
    }
}
