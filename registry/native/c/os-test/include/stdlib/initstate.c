/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <stdlib.h>
#ifdef initstate
#undef initstate
#endif
char *(*foo)(unsigned, char *, size_t) = initstate;
int main(void) { return 0; }
