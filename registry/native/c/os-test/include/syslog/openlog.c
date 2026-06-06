/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <syslog.h>
#ifdef openlog
#undef openlog
#endif
void (*foo)(const char *, int, int) = openlog;
int main(void) { return 0; }
