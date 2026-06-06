/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/time.h>
#ifndef FD_CLR
#error "FD_CLR is not defined"
#endif
int main(void) { return 0; }
