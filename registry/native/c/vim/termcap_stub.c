/* termcap_stub.c — minimal termcap API so vim links and falls back to its
 * builtin terminal entries. tgetent() returns 0 ("no entry") which makes vim
 * use its compiled-in builtin termcap for $TERM. tputs writes capability
 * strings straight to the output fn (vim passes its own putc). */
#include <stddef.h>
#include <unistd.h>

char PC = '\0';
char *UP = (char *)0;
char *BC = (char *)0;
short ospeed = 0;

int tgetent(char *bp, const char *name) {
	(void)bp;
	(void)name;
	return 0; /* no external entry -> vim uses builtin termcap */
}

int tgetflag(const char *id) {
	(void)id;
	return 0;
}

int tgetnum(const char *id) {
	(void)id;
	return -1;
}

char *tgetstr(const char *id, char **area) {
	(void)id;
	(void)area;
	return (char *)0;
}

/* tgoto — expand a parameterized termcap capability (cursor motion T_CM,
 * scroll region T_CS, etc.). With HAVE_TGETENT defined, vim routes ALL
 * parameterized caps through this external tgoto; a NULL/stub return silently
 * drops the escape (so t_cm/t_cs are set but never emitted → linear draw, no
 * scroll region, status line on the wrong row). This is vim's own minimal
 * tgoto (src/term.c, the #ifndef HAVE_TGETENT branch): parse %i, %d, %+char,
 * %%; termcap convention is tgoto(cap, destcol, destline) with %d emitting the
 * LINE first, then swapping to the column. */
char *tgoto(const char *cap, int col, int line) {
	static char buf[64];
	char *s = buf;
	char *e = buf + sizeof(buf) - 1;
	int x = col;
	int y = line;

	if (!cap) {
		return "OOPS";
	}
	for (; s < e && *cap; cap++) {
		if (*cap != '%') {
			*s++ = *cap;
			continue;
		}
		switch (*++cap) {
		case 'd': {
			/* emit y as decimal, then swap so the next %d emits x */
			char num[16];
			int n = 0;
			unsigned v = (unsigned)(y < 0 ? 0 : y);
			do {
				num[n++] = (char)('0' + v % 10);
				v /= 10;
			} while (v && n < (int)sizeof(num));
			while (n > 0 && s < e) {
				*s++ = num[--n];
			}
			y = x;
			break;
		}
		case 'i':
			x++;
			y++;
			break;
		case '+':
			*s++ = (char)(*++cap + y);
			y = x;
			break;
		case '%':
			*s++ = '%';
			break;
		default:
			return "OOPS";
		}
	}
	*s = '\0';
	return buf;
}

int tputs(const char *str, int affcnt, int (*putc_fn)(int)) {
	(void)affcnt;
	if (str && putc_fn) {
		for (const char *p = str; *p; p++) {
			putc_fn((int)(unsigned char)*p);
		}
	}
	return 0;
}
