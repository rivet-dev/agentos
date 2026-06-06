/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <unistd.h>
#ifdef swab
#undef swab
#endif
void (*foo)(const void *restrict, void *restrict, ssize_t) = swab;
int main(void) { return 0; }
