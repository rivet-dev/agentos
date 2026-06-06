/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <syslog.h>
#ifdef closelog
#undef closelog
#endif
void (*foo)(void) = closelog;
int main(void) { return 0; }
