/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <libgen.h>
#ifdef basename
#undef basename
#endif
char *(*foo)(char *) = basename;
int main(void) { return 0; }
