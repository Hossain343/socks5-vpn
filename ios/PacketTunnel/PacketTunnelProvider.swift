import NetworkExtension

/// Called by the Rust engine for every packet destined to the device (any thread).
private let packetOut: vpn_packet_cb = { data, len, ctx in
    guard let data = data, let ctx = ctx else { return }
    let provider = Unmanaged<PacketTunnelProvider>.fromOpaque(ctx).takeUnretainedValue()
    let pkt = Data(bytes: data, count: len)
    let family: Int32 = (data.pointee >> 4) == 6 ? AF_INET6 : AF_INET
    provider.packetFlow.writePackets([pkt], withProtocols: [NSNumber(value: family)])
}

final class PacketTunnelProvider: NEPacketTunnelProvider {
    private var running = false
    private var watchdog: DispatchSourceTimer?

    // Tunnel addresses sit outside private ranges so they never collide with a LAN.
    private let tunAddress = "198.19.0.2"
    private let dnsAddress = "198.19.0.1"

    private let lanRoutes: [(String, String)] = [
        ("10.0.0.0", "255.0.0.0"), ("100.64.0.0", "255.192.0.0"), ("127.0.0.0", "255.0.0.0"),
        ("169.254.0.0", "255.255.0.0"), ("172.16.0.0", "255.240.0.0"),
        ("192.168.0.0", "255.255.0.0"), ("224.0.0.0", "240.0.0.0"),
    ]

    override func startTunnel(options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        guard let proto = protocolConfiguration as? NETunnelProviderProtocol,
              let conf = proto.providerConfiguration,
              let host = conf["host"] as? String, !host.isEmpty,
              let port = conf["port"] as? Int, (1...65535).contains(port) else {
            completionHandler(fail(1, "تنظیمات سرور ناقص است"))
            return
        }
        let user = conf["user"] as? String ?? ""
        let pass = conf["pass"] as? String ?? ""

        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: host)
        let v4 = NEIPv4Settings(addresses: [tunAddress], subnetMasks: ["255.255.255.0"])
        v4.includedRoutes = [NEIPv4Route.default()]
        v4.excludedRoutes = lanRoutes.map { NEIPv4Route(destinationAddress: $0.0, subnetMask: $0.1) }
        settings.ipv4Settings = v4
        let dns = NEDNSSettings(servers: [dnsAddress])
        dns.matchDomains = [""] // send every lookup to the local fake-DNS
        settings.dnsSettings = dns
        settings.mtu = 1500

        setTunnelNetworkSettings(settings) { [weak self] error in
            guard let self = self else { return }
            if let error = error { completionHandler(error); return }
            let ctx = Unmanaged.passUnretained(self).toOpaque()
            if vpn_start(host, UInt16(port), user, pass, packetOut, ctx) != 0 {
                var buf = [CChar](repeating: 0, count: 256)
                _ = vpn_last_error(&buf, buf.count)
                let msg = String(cString: buf)
                completionHandler(self.fail(2, msg.isEmpty ? "شروع هسته ناموفق بود" : msg))
                return
            }
            self.running = true
            self.readPackets()
            self.startWatchdog()
            completionHandler(nil)
        }
    }

    override func stopTunnel(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        running = false
        watchdog?.cancel()
        watchdog = nil
        vpn_stop()
        completionHandler()
    }

    override func handleAppMessage(_ messageData: Data, completionHandler: ((Data?) -> Void)?) {
        var a = [Int64](repeating: 0, count: 5)
        vpn_stats(&a)
        let json = "{\"c\":\(a[0]),\"up\":\(a[1]),\"down\":\(a[2]),\"s\":\(a[3]),\"e\":\(a[4])}"
        completionHandler?(json.data(using: .utf8))
    }

    private func readPackets() {
        packetFlow.readPackets { [weak self] packets, _ in
            guard let self = self, self.running else { return }
            for p in packets {
                p.withUnsafeBytes { raw in
                    if let base = raw.bindMemory(to: UInt8.self).baseAddress {
                        vpn_input(base, raw.count)
                    }
                }
            }
            self.readPackets()
        }
    }

    /// If the engine dies (e.g. unreachable server), tear the tunnel down so the app shows "disconnected".
    private func startWatchdog() {
        let t = DispatchSource.makeTimerSource(queue: .global())
        t.schedule(deadline: .now() + 2, repeating: 2)
        t.setEventHandler { [weak self] in
            var a = [Int64](repeating: 0, count: 5)
            vpn_stats(&a)
            if a[0] == 0 {
                var buf = [CChar](repeating: 0, count: 256)
                _ = vpn_last_error(&buf, buf.count)
                let msg = String(cString: buf)
                self?.cancelTunnelWithError(self?.fail(3, msg.isEmpty ? "اتصال قطع شد" : msg))
            }
        }
        t.resume()
        watchdog = t
    }

    private func fail(_ code: Int, _ msg: String) -> NSError {
        NSError(domain: "VpnClient", code: code, userInfo: [NSLocalizedDescriptionKey: msg])
    }
}
