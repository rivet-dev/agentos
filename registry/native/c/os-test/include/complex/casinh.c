#include <complex.h>
#ifdef casinh
#undef casinh
#endif
double complex (*foo)(double complex) = casinh;
int main(void) { return 0; }
