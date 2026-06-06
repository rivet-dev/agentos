/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <strings.h>
#ifdef ffsll
#undef ffsll
#endif
int (*foo)(long long) = ffsll;
int main(void) { return 0; }
