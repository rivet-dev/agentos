/**
 * Fix for wasi-libc's open_wmemstream size reporting.
 *
 * musl's open_wmemstream reports size in bytes instead of wide characters.
 * This fix uses fopencookie to maintain a wchar_t buffer directly,
 * reporting size as wchar_t count per POSIX.
 *
 * Installed into the patched sysroot so ALL WASM programs get correct
 * open_wmemstream behavior, not just test binaries.
 */

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>
#include <errno.h>

struct wmem_cookie {
    wchar_t **bufp;     /* user's buffer pointer */
    size_t  *sizep;     /* user's size pointer (wide char count) */
    wchar_t *buf;       /* internal buffer */
    size_t  cap;        /* capacity in wchar_t units */
    size_t  len;        /* current length in wchar_t units */
    size_t  pos;        /* current position in wchar_t units */
};

static ssize_t wmem_write(void *cookie, const char *data, size_t nbytes) {
    struct wmem_cookie *wm = (struct wmem_cookie *)cookie;

    /* fwprintf writes multibyte (UTF-8) through the FILE.  We need to
       convert back to wide chars and store in our wchar_t buffer.
       For ASCII (which covers most use cases), each byte = one wchar_t. */
    size_t nchars = 0;
    const char *src = data;
    const char *end = data + nbytes;
    mbstate_t mbs;
    memset(&mbs, 0, sizeof(mbs));

    /* First pass: count wide chars needed */
    const char *tmp = src;
    mbstate_t tmps;
    memset(&tmps, 0, sizeof(tmps));
    while (tmp < end) {
        wchar_t wc;
        size_t r = mbrtowc(&wc, tmp, end - tmp, &tmps);
        if (r == (size_t)-1 || r == (size_t)-2) {
            /* Invalid or incomplete sequence -- treat remaining as individual bytes */
            nchars += end - tmp;
            break;
        }
        if (r == 0) r = 1; /* null byte */
        nchars++;
        tmp += r;
    }

    /* Ensure buffer capacity */
    size_t needed = wm->pos + nchars + 1; /* +1 for null terminator */
    if (needed > wm->cap) {
        size_t newcap = wm->cap ? wm->cap * 2 : 64;
        while (newcap < needed) newcap *= 2;
        wchar_t *newbuf = realloc(wm->buf, newcap * sizeof(wchar_t));
        if (!newbuf) return -1;
        wm->buf = newbuf;
        wm->cap = newcap;
    }

    /* Second pass: convert and store */
    src = data;
    memset(&mbs, 0, sizeof(mbs));
    while (src < end) {
        wchar_t wc;
        size_t r = mbrtowc(&wc, src, end - src, &mbs);
        if (r == (size_t)-1 || r == (size_t)-2) {
            /* Fallback: store raw bytes as wchar_t */
            while (src < end) {
                wm->buf[wm->pos++] = (wchar_t)(unsigned char)*src++;
            }
            break;
        }
        if (r == 0) { wc = L'\0'; r = 1; }
        wm->buf[wm->pos++] = wc;
        src += r;
    }

    /* Update length */
    if (wm->pos > wm->len) wm->len = wm->pos;

    /* Null terminate and update user pointers */
    wm->buf[wm->len] = L'\0';
    *wm->bufp = wm->buf;
    *wm->sizep = wm->len;

    return nbytes;
}

static int wmem_close(void *cookie) {
    struct wmem_cookie *wm = (struct wmem_cookie *)cookie;
    /* Null terminate final buffer */
    if (wm->buf) {
        if (wm->len >= wm->cap) {
            wchar_t *newbuf = realloc(wm->buf, (wm->len + 1) * sizeof(wchar_t));
            if (newbuf) wm->buf = newbuf;
        }
        wm->buf[wm->len] = L'\0';
    }
    *wm->bufp = wm->buf;
    *wm->sizep = wm->len;
    free(wm);
    return 0;
}

FILE *open_wmemstream(wchar_t **bufp, size_t *sizep) {
    struct wmem_cookie *wm = calloc(1, sizeof(*wm));
    if (!wm) return NULL;

    wm->bufp = bufp;
    wm->sizep = sizep;
    wm->cap = 64;
    wm->buf = calloc(wm->cap, sizeof(wchar_t));
    if (!wm->buf) { free(wm); return NULL; }

    *bufp = wm->buf;
    *sizep = 0;

    cookie_io_functions_t funcs = {
        .read = NULL,
        .write = wmem_write,
        .seek = NULL,
        .close = wmem_close,
    };

    FILE *fp = fopencookie(wm, "w", funcs);
    if (!fp) {
        free(wm->buf);
        free(wm);
        return NULL;
    }

    /* Set wide orientation */
    fwide(fp, 1);

    return fp;
}
