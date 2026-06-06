/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <signal.h>
#ifdef sigaltstack
#undef sigaltstack
#endif
int (*foo)(const stack_t *restrict, stack_t *restrict) = sigaltstack;
int main(void) { return 0; }
