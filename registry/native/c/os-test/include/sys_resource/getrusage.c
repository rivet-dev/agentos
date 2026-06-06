/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/resource.h>
#ifdef getrusage
#undef getrusage
#endif
int (*foo)(int, struct rusage *) = getrusage;
int main(void) { return 0; }
