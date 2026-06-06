/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <ftw.h>
#ifndef S_ISFIFO
#error "S_ISFIFO is not defined"
#endif
int main(void) { return 0; }
