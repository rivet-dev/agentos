/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/resource.h>
#ifdef getpriority
#undef getpriority
#endif
int (*foo)(int, id_t) = getpriority;
int main(void) { return 0; }
