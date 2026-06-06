/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <stdlib.h>
#ifdef posix_openpt
#undef posix_openpt
#endif
int (*foo)(int) = posix_openpt;
int main(void) { return 0; }
