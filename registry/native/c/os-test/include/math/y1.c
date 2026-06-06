/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <math.h>
#ifdef y1
#undef y1
#endif
double (*foo)(double) = y1;
int main(void) { return 0; }
