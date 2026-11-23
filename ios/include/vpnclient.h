#ifndef VPNCLIENT_H
#define VPNCLIENT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Called for every IP packet the engine wants to write to the device. May run on any thread. */
typedef void (*vpn_packet_cb)(const uint8_t *data, size_t len, void *ctx);

/* 0 = ok, -1 = failure (see vpn_last_error). */
int32_t vpn_start(const char *host, uint16_t port, const char *user, const char *pass,
                  vpn_packet_cb cb, void *ctx);

/* Feed one IP packet read from NEPacketTunnelFlow. Non-blocking. */
void vpn_input(const uint8_t *data, size_t len);

void vpn_stop(void);

/* out5 = { connected(0/1), bytesUp, bytesDown, activeSessions, errors } */
void vpn_stats(int64_t *out5);

/* Copies the last error (NUL-terminated) into buf; returns bytes written. */
size_t vpn_last_error(char *buf, size_t cap);

#ifdef __cplusplus
}
#endif

#endif
