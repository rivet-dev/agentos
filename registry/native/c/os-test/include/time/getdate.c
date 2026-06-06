/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <time.h>
#ifdef getdate
#undef getdate
#endif
struct tm *(*foo)(const char *) = getdate;
int main(void) { return 0; }
