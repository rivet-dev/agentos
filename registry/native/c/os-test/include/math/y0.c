/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <math.h>
#ifdef y0
#undef y0
#endif
double (*foo)(double) = y0;
int main(void) { return 0; }
