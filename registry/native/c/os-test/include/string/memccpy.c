/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <string.h>
#ifdef memccpy
#undef memccpy
#endif
void *(*foo)(void *restrict, const void *restrict, int, size_t) = memccpy;
int main(void) { return 0; }
