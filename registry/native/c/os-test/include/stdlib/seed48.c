/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <stdlib.h>
#ifdef seed48
#undef seed48
#endif
unsigned short *(*foo)(unsigned short [3]) = seed48;
int main(void) { return 0; }
