/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/stat.h>
#ifdef mknodat
#undef mknodat
#endif
int (*foo)(int, const char *, mode_t, dev_t) = mknodat;
int main(void) { return 0; }
