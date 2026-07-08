/**
 * fmtmsg() override for wasi-libc.
 *
 * musl's fmtmsg() is a no-op stub (returns 0 without writing).
 * This provides a POSIX-conformant implementation that writes
 * formatted messages to stderr (MM_PRINT) per POSIX.1-2024.
 *
 * Format: "label: severity: text\nTO FIX: action tag\n"
 *
 * Classification flags (MM_HARD/SOFT/FIRM, MM_APPL/UTIL/OPSYS,
 * MM_RECOVER/MM_NRECOV) are accepted per POSIX. MM_PRINT and
 * MM_CONSOLE control output destination; the remaining flags are
 * classification metadata that does not alter the output format.
 */

#include <fmtmsg.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

int fmtmsg(long classification, const char *label, int severity,
           const char *text, const char *action, const char *tag)
{
    int print = (classification & MM_PRINT);
    int console = (classification & MM_CONSOLE);

    /* Validate classification: at most one recoverability flag */
    if ((classification & MM_RECOVER) && (classification & MM_NRECOV))
        return MM_NOTOK;

    /* Determine severity string */
    const char *sev;
    switch (severity) {
    case MM_HALT:    sev = "HALT";    break;
    case MM_ERROR:   sev = "ERROR";   break;
    case MM_WARNING: sev = "WARNING"; break;
    case MM_INFO:    sev = "INFO";    break;
    default:         sev = "UNKNOWN"; break;
    }

    /* Calculate buffer size proportional to inputs */
    size_t need = 64; /* overhead for "label: severity: text\nTO FIX: action tag\n" framing */
    if (label && label != MM_NULLLBL)
        need += strlen(label);
    need += strlen(sev);
    if (text && text != MM_NULLTXT)
        need += strlen(text);
    if (action && action != MM_NULLACT)
        need += strlen(action);
    if (tag && tag != MM_NULLTAG)
        need += strlen(tag);

    char *buf = (char *)malloc(need);
    if (!buf)
        return MM_NOTOK;

    int len = 0;

    /* Line 1: label: severity: text */
    if (label && label != MM_NULLLBL)
        len += snprintf(buf + len, need - len, "%s: ", label);
    len += snprintf(buf + len, need - len, "%s: ", sev);
    if (text && text != MM_NULLTXT)
        len += snprintf(buf + len, need - len, "%s", text);
    len += snprintf(buf + len, need - len, "\n");

    /* Line 2: TO FIX: action tag */
    if ((action && action != MM_NULLACT) || (tag && tag != MM_NULLTAG)) {
        len += snprintf(buf + len, need - len, "TO FIX: ");
        if (action && action != MM_NULLACT)
            len += snprintf(buf + len, need - len, "%s", action);
        if (tag && tag != MM_NULLTAG)
            len += snprintf(buf + len, need - len, " %s", tag);
        len += snprintf(buf + len, need - len, "\n");
    }

    int result = MM_OK;

    if (print) {
        if (fputs(buf, stderr) == EOF || fflush(stderr) == EOF)
            result = MM_NOMSG;
    }

    /* MM_CONSOLE: on real Linux writes to /dev/console.
       In WASM sandbox we just write to stderr as well. */
    if (console && result == MM_OK) {
        /* Already written above if MM_PRINT was also set */
        if (!print) {
            if (fputs(buf, stderr) == EOF || fflush(stderr) == EOF)
                result = MM_NOCON;
        }
    }

    free(buf);
    return result;
}
