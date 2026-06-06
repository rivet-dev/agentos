/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <limits.h>
#ifndef _XOPEN_PATH_MAX
#error "_XOPEN_PATH_MAX is not defined"
#endif
int main(void) { return 0; }
