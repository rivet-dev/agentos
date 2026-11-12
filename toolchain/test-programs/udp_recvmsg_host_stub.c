/* Inject only the host route selector: ENOTSOCK is the real host-net fallback.
 * The owning recvmsg implementation and Linux UDP recv/recvfrom remain real. */
#include <stdint.h>
#include <errno.h>
uint32_t __host_fd_recvmsg_rights(uint32_t fd,uint8_t *data,uint32_t len,uint32_t *rights,uint32_t cap,uint32_t flags,uint32_t *received,uint32_t *nrights,uint32_t *outflags) {
 (void)fd;(void)data;(void)len;(void)rights;(void)cap;(void)flags;(void)received;(void)nrights;(void)outflags;return ENOTSOCK;
}
int __agentos_set_cloexec_fd(int fd,int enabled){(void)fd;(void)enabled;return 0;}

struct msghdr;
uint32_t __host_net_recvmsg(uint32_t fd, struct msghdr *message, uint32_t flags, uint32_t *received) {
 (void)fd;(void)message;(void)flags;(void)received;return ENOTSOCK;
}
