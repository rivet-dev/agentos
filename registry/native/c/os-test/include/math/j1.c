/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <math.h>
#ifdef j1
#undef j1
#endif
double (*foo)(double) = j1;
int main(void) { return 0; }
