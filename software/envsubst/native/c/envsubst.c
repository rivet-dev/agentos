/*
 * envsubst - substitute environment variables in stdin
 *
 * Reads stdin, replaces $VAR and ${VAR} references with their
 * environment variable values. Undefined variables become empty string.
 * Supports ${VAR:-default} for default values and \$VAR for literal $.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>

/* Valid variable name character: [A-Za-z0-9_] */
static int is_varchar(int c) {
    return isalnum(c) || c == '_';
}

/* Read a variable name from src into buf (max bufsize-1 chars).
 * Returns number of chars consumed from src. */
static int read_varname(const char *src, char *buf, int bufsize) {
    int i = 0;
    while (src[i] && is_varchar((unsigned char)src[i]) && i < bufsize - 1) {
        buf[i] = src[i];
        i++;
    }
    buf[i] = '\0';
    return i;
}

int main(void) {
    char line[8192];

    while (fgets(line, sizeof(line), stdin)) {
        const char *p = line;

        while (*p) {
            /* Escaped dollar sign */
            if (*p == '\\' && *(p + 1) == '$') {
                putchar('$');
                p += 2;
                continue;
            }

            if (*p != '$') {
                putchar(*p);
                p++;
                continue;
            }

            /* $ found */
            p++; /* skip '$' */

            if (*p == '{') {
                /* ${VAR} or ${VAR:-default} */
                p++; /* skip '{' */
                char varname[256];
                int n = read_varname(p, varname, sizeof(varname));
                p += n;

                char *defval = NULL;

                /* Check for :-default */
                if (p[0] == ':' && p[1] == '-') {
                    p += 2; /* skip ':-' */
                    /* Read default value until '}' */
                    char defbuf[4096];
                    int di = 0;
                    while (*p && *p != '}' && di < (int)sizeof(defbuf) - 1) {
                        defbuf[di++] = *p++;
                    }
                    defbuf[di] = '\0';
                    defval = defbuf;
                }

                /* Skip closing '}' */
                if (*p == '}') p++;

                const char *val = getenv(varname);
                if (val) {
                    fputs(val, stdout);
                } else if (defval) {
                    fputs(defval, stdout);
                }
                /* else: undefined with no default → empty string */
            } else if (is_varchar((unsigned char)*p)) {
                /* $VAR */
                char varname[256];
                int n = read_varname(p, varname, sizeof(varname));
                p += n;

                const char *val = getenv(varname);
                if (val) {
                    fputs(val, stdout);
                }
                /* else: undefined → empty string */
            } else {
                /* Lone $ followed by non-varchar — output literally */
                putchar('$');
            }
        }
    }

    return 0;
}
