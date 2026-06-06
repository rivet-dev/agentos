/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <strings.h>
#ifdef ffsl
#undef ffsl
#endif
int (*foo)(long) = ffsl;
int main(void) { return 0; }
