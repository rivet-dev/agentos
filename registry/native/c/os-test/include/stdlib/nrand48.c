/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <stdlib.h>
#ifdef nrand48
#undef nrand48
#endif
long (*foo)(unsigned short [3]) = nrand48;
int main(void) { return 0; }
