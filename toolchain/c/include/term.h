#ifndef AGENTOS_C_INCLUDE_TERM_H
#define AGENTOS_C_INCLUDE_TERM_H

#include <curses.h>

int tgetent(char *buffer, const char *termtype);
char *tgetstr(const char *id, char **area);

#endif
