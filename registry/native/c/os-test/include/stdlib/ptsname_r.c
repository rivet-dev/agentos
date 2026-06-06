/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <stdlib.h>
#ifdef ptsname_r
#undef ptsname_r
#endif
int (*foo)(int, char *, size_t) = ptsname_r;
int main(void) { return 0; }
