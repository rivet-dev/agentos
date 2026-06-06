/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <math.h>
#ifdef jn
#undef jn
#endif
double (*foo)(int, double) = jn;
int main(void) { return 0; }
