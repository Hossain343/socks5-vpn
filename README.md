# VPN Share client (Rust core + Android + iOS)

Rust engine (`rust/`) = TUN -> userspace TCP/UDP stack (netstack-smoltcp) -> SOCKS5 (CONNECT + UDP ASSOCIATE,
RFC 1929 auth) against vpnshare-rs, with local fake-DNS so the server still sees real domain names.

* `android/` Kotlin app + VpnService + widget, loads `libvpnclient.so` via JNI.
* `ios/` SwiftUI app + `NEPacketTunnelProvider`, links `libvpnclient.a` via the C ABI in `ios/include/vpnclient.h`.

## CI (GitHub Actions)
* `.github/workflows/android.yml`: rust check + tests -> `cargo ndk` (3 ABIs) -> Gradle -> APK artifacts.
* `.github/workflows/ios.yml`: static libs for device + simulator -> XCFramework -> XcodeGen -> unsigned IPA.

Push to GitHub, open the Actions tab, download artifacts. `Cargo.lock` is not included: run
`cargo generate-lockfile` in `rust/` once and commit it for reproducible builds.

## iOS signing
The IPA is unsigned. A Packet Tunnel extension needs the *Network Extension* capability
(paid Apple Developer account, App IDs `com.vpnshare.client` and `com.vpnshare.client.tunnel`, both with
"Network Extensions" enabled) and a provisioning profile for each. Re-sign the IPA with your own
certificate/profiles. The `ldid` IPA embeds the entitlements for installers that accept pseudo-signing (e.g. TrollStore).

## Local Android build
```bash
cd rust && cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o ../android/app/src/main/jniLibs build --release
cd ../android && gradle assembleDebug
```

## Test against vpnshare-rs
1. Settings: host, port (+ user/pass), Apply. Then tap the big button.
2. Android: `adb logcat -s vpnclient`. TCP: browse; the server log shows domains (not 198.18.x.x).
3. UDP: open an HTTP/3 site (QUIC); server should log a UDP ASSOCIATE. If it answers 0.0.0.0 the client uses the server IP it connected to.
4. DNS: `nslookup example.com` returns 198.18.x.x (fake pool) - expected.
