/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <stdlib.h>
#ifdef jrand48
#undef jrand48
#endif
long (*foo)(unsigned short [3]) = jrand48;
int main(void) { return 0; }
