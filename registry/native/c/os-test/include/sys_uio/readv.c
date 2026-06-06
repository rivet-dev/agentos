/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/uio.h>
#ifdef readv
#undef readv
#endif
ssize_t (*foo)(int, const struct iovec *, int) = readv;
int main(void) { return 0; }
