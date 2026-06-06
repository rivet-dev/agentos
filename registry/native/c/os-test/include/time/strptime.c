/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <time.h>
#ifdef strptime
#undef strptime
#endif
char *(*foo)(const char *restrict, const char *restrict, struct tm *restrict) = strptime;
int main(void) { return 0; }
