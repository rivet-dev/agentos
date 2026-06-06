/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <stdlib.h>
#ifdef erand48
#undef erand48
#endif
double (*foo)(unsigned short [3]) = erand48;
int main(void) { return 0; }
