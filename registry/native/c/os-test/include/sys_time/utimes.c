/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/time.h>
#ifdef utimes
#undef utimes
#endif
int (*foo)(const char *, const struct timeval [2]) = utimes;
int main(void) { return 0; }
