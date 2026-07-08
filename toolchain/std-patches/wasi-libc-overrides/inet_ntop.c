/**
 * Fix for musl's inet_ntop() IPv6 formatting.
 *
 * musl does not fully comply with RFC 5952:
 * - When two zero-runs have equal length, musl may pick the wrong one
 *   (RFC 5952 requires the leftmost run)
 * - Single zero fields should not be compressed with ::
 *
 * This fix provides RFC 5952 compliant IPv6 formatting.
 *
 * Installed into the patched sysroot so ALL WASM programs get correct
 * inet_ntop behavior, not just test binaries.
 */

#include <stdio.h>
#include <string.h>
#include <errno.h>
#include <arpa/inet.h>

static const char *inet_ntop4(const unsigned char *src, char *dst, socklen_t size) {
    int n = snprintf(dst, size, "%d.%d.%d.%d", src[0], src[1], src[2], src[3]);
    if (n < 0 || (socklen_t)n >= size) {
        errno = ENOSPC;
        return NULL;
    }
    return dst;
}

static const char *inet_ntop6(const unsigned char *src, char *dst, socklen_t size) {
    /* Parse 16 bytes into 8 groups */
    unsigned short groups[8];
    for (int i = 0; i < 8; i++) {
        groups[i] = ((unsigned short)src[i*2] << 8) | src[i*2+1];
    }

    /* Check for IPv4-mapped address (::ffff:x.x.x.x) */
    int is_v4mapped = (groups[0] == 0 && groups[1] == 0 && groups[2] == 0 &&
                       groups[3] == 0 && groups[4] == 0 && groups[5] == 0xffff);

    /* Find the longest run of consecutive zeros (RFC 5952 Section 4.2.3)
     * - Must be at least 2 groups long
     * - If tied, use the leftmost run (RFC 5952 Section 4.2.3 #1) */
    int best_start = -1, best_len = 0;
    int cur_start = -1, cur_len = 0;

    int limit = is_v4mapped ? 6 : 8;
    for (int i = 0; i < limit; i++) {
        if (groups[i] == 0) {
            if (cur_start < 0) cur_start = i;
            cur_len++;
        } else {
            if (cur_len > best_len && cur_len >= 2) {
                best_start = cur_start;
                best_len = cur_len;
            }
            cur_start = -1;
            cur_len = 0;
        }
    }
    if (cur_len > best_len && cur_len >= 2) {
        best_start = cur_start;
        best_len = cur_len;
    }

    /* Format the address */
    char buf[64];
    char *p = buf;

    if (is_v4mapped) {
        /* ::ffff:a.b.c.d */
        p += sprintf(p, "::ffff:");
        p += sprintf(p, "%d.%d.%d.%d", src[12], src[13], src[14], src[15]);
    } else {
        int after_gap = 0;
        for (int i = 0; i < 8; i++) {
            if (i == best_start) {
                p += sprintf(p, "::");
                i += best_len - 1;
                after_gap = 1;
                continue;
            }
            if (i > 0 && !after_gap) *p++ = ':';
            after_gap = 0;
            p += sprintf(p, "%x", groups[i]);
        }
    }
    *p = '\0';

    size_t len = p - buf + 1;
    if (len > size) {
        errno = ENOSPC;
        return NULL;
    }
    memcpy(dst, buf, len);
    return dst;
}

const char *inet_ntop(int af, const void *restrict src,
                      char *restrict dst, socklen_t size) {
    if (af == AF_INET) {
        return inet_ntop4((const unsigned char *)src, dst, size);
    }
#ifdef AF_INET6
    if (af == AF_INET6) {
        return inet_ntop6((const unsigned char *)src, dst, size);
    }
#endif
    errno = EAFNOSUPPORT;
    return NULL;
}
