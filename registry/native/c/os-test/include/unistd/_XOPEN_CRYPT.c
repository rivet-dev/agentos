/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <unistd.h>
#ifndef _XOPEN_CRYPT
#error "_XOPEN_CRYPT is not defined"
#endif
int main(void) { return 0; }
