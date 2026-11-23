import Foundation
import NetworkExtension

/// Owns the NETunnelProviderManager. The server settings live in the VPN configuration
/// (providerConfiguration) and are written ONLY by `apply(...)` - never automatically.
final class VpnManager: ObservableObject {
    static let tunnelBundleId = "com.vpnshare.client.tunnel"

    @Published var status: NEVPNStatus = .disconnected
    @Published var loaded = false
    @Published var host = ""
    @Published var port = 1080
    @Published var user = ""
    @Published var pass = ""
    @Published var error: String?
    @Published var upSpeed: Int64 = 0
    @Published var downSpeed: Int64 = 0
    @Published var totalUp: Int64 = 0
    @Published var totalDown: Int64 = 0

    private var manager: NETunnelProviderManager?
    private var timer: Timer?
    private var lastUp: Int64 = 0
    private var lastDown: Int64 = 0

    init() {
        NotificationCenter.default.addObserver(forName: .NEVPNStatusDidChange, object: nil, queue: .main) { [weak self] _ in
            self?.refreshStatus()
        }
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in self?.pollStats() }
        load()
    }

    func load() {
        NETunnelProviderManager.loadAllFromPreferences { [weak self] managers, _ in
            DispatchQueue.main.async {
                guard let self = self else { return }
                self.manager = managers?.first
                self.readConfig()
                self.loaded = true
                self.refreshStatus()
            }
        }
    }

    private func readConfig() {
        guard let p = manager?.protocolConfiguration as? NETunnelProviderProtocol,
              let c = p.providerConfiguration else { return }
        host = c["host"] as? String ?? ""
        port = c["port"] as? Int ?? 1080
        user = c["user"] as? String ?? ""
        pass = c["pass"] as? String ?? ""
    }

    func apply(host: String, port: Int, user: String, pass: String, done: @escaping (String?) -> Void) {
        let m = manager ?? NETunnelProviderManager()
        let p = NETunnelProviderProtocol()
        p.providerBundleIdentifier = VpnManager.tunnelBundleId
        p.serverAddress = host
        p.providerConfiguration = ["host": host, "port": port, "user": user, "pass": pass]
        m.protocolConfiguration = p
        m.localizedDescription = "VPN"
        m.isEnabled = true
        m.saveToPreferences { err in
            if let err = err {
                DispatchQueue.main.async { done(err.localizedDescription) }
                return
            }
            m.loadFromPreferences { err in
                DispatchQueue.main.async {
                    self.manager = m
                    self.readConfig()
                    done(err?.localizedDescription)
                }
            }
        }
    }

    func toggle() {
        switch status {
        case .connected, .connecting, .reasserting:
            manager?.connection.stopVPNTunnel()
        default:
            guard let m = manager, !host.isEmpty else {
                error = "ابتدا تنظیمات را ذخیره کنید"
                return
            }
            error = nil
            do { try m.connection.startVPNTunnel() } catch let err { error = err.localizedDescription }
        }
    }

    private func refreshStatus() {
        status = manager?.connection.status ?? .disconnected
        if status != .connected {
            upSpeed = 0; downSpeed = 0; lastUp = 0; lastDown = 0
        }
    }

    private func pollStats() {
        guard status == .connected, let s = manager?.connection as? NETunnelProviderSession else { return }
        try? s.sendProviderMessage(Data("stats".utf8)) { [weak self] data in
            guard let self = self, let data = data,
                  let j = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
            let up = (j["up"] as? NSNumber)?.int64Value ?? 0
            let down = (j["down"] as? NSNumber)?.int64Value ?? 0
            DispatchQueue.main.async {
                self.upSpeed = max(0, up - self.lastUp)
                self.downSpeed = max(0, down - self.lastDown)
                self.lastUp = up; self.lastDown = down
                self.totalUp = up; self.totalDown = down
            }
        }
    }
}
