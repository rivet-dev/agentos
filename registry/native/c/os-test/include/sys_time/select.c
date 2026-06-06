/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/time.h>
#ifdef select
#undef select
#endif
int (*foo)(int, fd_set *restrict, fd_set *restrict, fd_set *restrict, struct timeval *restrict) = select;
int main(void) { return 0; }
