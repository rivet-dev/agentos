#ifndef AGENTOS_C_INCLUDE_CURSES_H
#define AGENTOS_C_INCLUDE_CURSES_H

#define OK 0
#define ERR (-1)

int setupterm(const char *term, int filedes, int *errret);
char *tigetstr(const char *capname);
int tputs(const char *str, int affcnt, int (*putc_fn)(int));

#endif
