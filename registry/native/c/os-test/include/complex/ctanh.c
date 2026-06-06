#include <complex.h>
#ifdef ctanh
#undef ctanh
#endif
double complex (*foo)(double complex) = ctanh;
int main(void) { return 0; }
