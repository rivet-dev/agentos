#include <complex.h>
#ifdef csinh
#undef csinh
#endif
double complex (*foo)(double complex) = csinh;
int main(void) { return 0; }
