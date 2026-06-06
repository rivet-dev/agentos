/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/stat.h>
#ifdef mknod
#undef mknod
#endif
int (*foo)(const char *, mode_t, dev_t) = mknod;
int main(void) { return 0; }
