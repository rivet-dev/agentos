#include <complex.h>
#ifdef csqrt
#undef csqrt
#endif
double complex (*foo)(double complex) = csqrt;
int main(void) { return 0; }
