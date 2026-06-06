#include <complex.h>
#ifdef clog
#undef clog
#endif
double complex (*foo)(double complex) = clog;
int main(void) { return 0; }
