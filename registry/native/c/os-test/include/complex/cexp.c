#include <complex.h>
#ifdef cexp
#undef cexp
#endif
double complex (*foo)(double complex) = cexp;
int main(void) { return 0; }
