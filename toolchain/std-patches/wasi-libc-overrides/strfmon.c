/**
 * Fix for wasi-libc's strfmon/strfmon_l for the POSIX locale.
 *
 * wasi-libc (musl) strfmon uses "." as mon_decimal_point even when
 * localeconv()->mon_decimal_point is "" (the POSIX locale value).
 * This fix implements strfmon per POSIX for the POSIX locale:
 *   - mon_decimal_point = "" (no decimal separator in output)
 *   - negative_sign = "-" when sign position is CHAR_MAX
 *   - No currency symbols
 *
 * Format: %[flags][width][#left_prec][.right_prec]{i|n}
 * Flags: =f (fill char), ^ (no grouping), + ( - ! (suppress currency)
 *
 * Installed into the patched sysroot so ALL WASM programs get correct
 * strfmon behavior, not just test binaries.
 */

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <locale.h>

static ssize_t vstrfmon_posix(char *s, size_t maxsize, const char *fmt, va_list ap) {
    char *out = s;
    char *end = s + maxsize;

    while (*fmt && out < end) {
        if (*fmt != '%') {
            *out++ = *fmt++;
            continue;
        }
        fmt++; /* skip '%' */

        /* %% -> literal % */
        if (*fmt == '%') {
            *out++ = '%';
            fmt++;
            continue;
        }

        /* Parse flags */
        char fill = ' ';
        int left_justify = 0;
        int no_grouping = 0;
        int paren_negative = 0;
        int plus_sign = 0;
        int suppress_currency = 0;

        for (;;) {
            if (*fmt == '=') {
                fmt++;
                fill = *fmt++;
            } else if (*fmt == '-') {
                left_justify = 1;
                fmt++;
            } else if (*fmt == '^') {
                no_grouping = 1;
                fmt++;
            } else if (*fmt == '(') {
                paren_negative = 1;
                fmt++;
            } else if (*fmt == '+') {
                plus_sign = 1;
                fmt++;
            } else if (*fmt == '!') {
                suppress_currency = 1;
                fmt++;
            } else {
                break;
            }
        }

        /* Parse field width */
        int width = 0;
        int has_width = 0;
        while (*fmt >= '0' && *fmt <= '9') {
            width = width * 10 + (*fmt - '0');
            has_width = 1;
            fmt++;
        }

        /* Parse left precision (#n) -- minimum digits to left of decimal */
        int left_prec = -1;
        if (*fmt == '#') {
            fmt++;
            left_prec = 0;
            while (*fmt >= '0' && *fmt <= '9') {
                left_prec = left_prec * 10 + (*fmt - '0');
                fmt++;
            }
        }

        /* Parse right precision (.n) -- digits to right of decimal */
        int right_prec = -1;
        if (*fmt == '.') {
            fmt++;
            right_prec = 0;
            while (*fmt >= '0' && *fmt <= '9') {
                right_prec = right_prec * 10 + (*fmt - '0');
                fmt++;
            }
        }

        /* Conversion character: 'i' (international) or 'n' (national) */
        char conv = *fmt++;
        if (conv != 'i' && conv != 'n') {
            errno = EINVAL;
            return -1;
        }

        (void)no_grouping;
        (void)suppress_currency;

        double val = va_arg(ap, double);
        int negative = val < 0;
        if (negative) val = -val;

        /* Default right precision: 2 for POSIX locale when not specified.
         * POSIX says use frac_digits/int_frac_digits from locale, but when
         * those are CHAR_MAX, the implementation chooses (we use 2). */
        if (right_prec < 0) right_prec = 2;

        /* Format the number without decimal point (POSIX locale: mon_decimal_point = "") */
        /* Multiply by 10^right_prec and round to get integer representation */
        double scale = 1.0;
        for (int i = 0; i < right_prec; i++) scale *= 10.0;
        long long ival = (long long)(val * scale + 0.5);

        /* Convert to digit string */
        char digits[64];
        int dlen = snprintf(digits, sizeof(digits), "%lld", ival);

        /* Apply left precision: pad with fill chars if needed */
        /* left_prec specifies minimum digits to the LEFT of where decimal would be */
        int left_digits = dlen - right_prec;
        if (left_digits < 0) left_digits = 0;
        int pad_count = 0;
        if (left_prec > 0 && left_digits < left_prec) {
            pad_count = left_prec - left_digits;
        }

        /* Build the formatted value into a temp buffer */
        char tmp[256];
        int tpos = 0;

        /* Sign handling: POSIX locale sign_posn = CHAR_MAX -> use "-" prefix */
        if (negative) {
            if (paren_negative) {
                tmp[tpos++] = '(';
            } else {
                tmp[tpos++] = '-';
            }
        } else if (plus_sign) {
            /* no positive sign in POSIX locale */
        }

        /* Fill chars for left precision padding */
        for (int i = 0; i < pad_count; i++) {
            tmp[tpos++] = fill;
        }

        /* The digits (includes both left and right parts, no decimal) */
        /* If total digits < right_prec, we need leading zeros */
        if (dlen <= right_prec) {
            /* Need leading zeros: e.g., val=0.01, right_prec=2, ival=1, dlen=1 */
            int need_zeros = right_prec - dlen + 1; /* +1 for at least one left digit */
            if (left_prec > 1) need_zeros = left_prec + right_prec - dlen;
            for (int i = 0; i < need_zeros && i < (int)sizeof(tmp) - tpos - dlen - 2; i++) {
                tmp[tpos++] = '0';
            }
        }
        memcpy(tmp + tpos, digits, dlen);
        tpos += dlen;

        if (negative && paren_negative) {
            tmp[tpos++] = ')';
        }

        tmp[tpos] = '\0';

        /* Apply field width */
        int vlen = tpos;
        if (has_width && width > vlen) {
            int padding = width - vlen;
            if (left_justify) {
                /* Value then spaces */
                for (int i = 0; i < vlen && out < end; i++) *out++ = tmp[i];
                for (int i = 0; i < padding && out < end; i++) *out++ = ' ';
            } else {
                /* Spaces then value */
                for (int i = 0; i < padding && out < end; i++) *out++ = ' ';
                for (int i = 0; i < vlen && out < end; i++) *out++ = tmp[i];
            }
        } else {
            for (int i = 0; i < vlen && out < end; i++) *out++ = tmp[i];
        }
    }

    if (out < end) *out = '\0';
    else if (maxsize > 0) s[maxsize - 1] = '\0';

    return (ssize_t)(out - s);
}

ssize_t strfmon(char *restrict s, size_t maxsize, const char *restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    ssize_t ret = vstrfmon_posix(s, maxsize, fmt, ap);
    va_end(ap);
    return ret;
}

ssize_t strfmon_l(char *restrict s, size_t maxsize, locale_t loc,
                  const char *restrict fmt, ...) {
    (void)loc; /* WASI only has the POSIX locale */
    va_list ap;
    va_start(ap, fmt);
    ssize_t ret = vstrfmon_posix(s, maxsize, fmt, ap);
    va_end(ap);
    return ret;
}
