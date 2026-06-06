/*[XSI|SIO]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/mman.h>
#ifdef msync
#undef msync
#endif
int (*foo)(void *, size_t, int) = msync;
int main(void) { return 0; }
