/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <syslog.h>
#ifdef syslog
#undef syslog
#endif
void (*foo)(int, const char *, ...) = syslog;
int main(void) { return 0; }
