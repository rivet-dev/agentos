#include <complex.h>
#ifdef cimag
#undef cimag
#endif
double (*foo)(double complex) = cimag;
int main(void) { return 0; }
