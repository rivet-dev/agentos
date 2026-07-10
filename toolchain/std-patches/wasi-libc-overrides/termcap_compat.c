#include <curses.h>
#include <stddef.h>
#include <stdio.h>
#include <term.h>

/* AgentOS VMs do not project a terminfo/termcap database. These APIs report
 * capability absence, which makes consumers use their documented ASCII path;
 * tputs still preserves explicitly supplied control strings.
 * https://man7.org/linux/man-pages/man5/termcap.5.html */
int setupterm(const char *term, int filedes, int *errret) {
	(void)term;
	(void)filedes;
	if (errret != NULL)
		*errret = -1;
	return ERR;
}

char *tigetstr(const char *capname) {
	(void)capname;
	return NULL;
}

int tgetent(char *buffer, const char *termtype) {
	(void)buffer;
	(void)termtype;
	return 0;
}

char *tgetstr(const char *id, char **area) {
	(void)id;
	(void)area;
	return NULL;
}

int tputs(const char *str, int affcnt, int (*putc_fn)(int)) {
	(void)affcnt;
	if (str == NULL || putc_fn == NULL)
		return ERR;
	for (; *str != '\0'; str++) {
		if (putc_fn((unsigned char)*str) == EOF)
			return ERR;
	}
	return OK;
}
