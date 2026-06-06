#include <complex.h>
#ifdef catan
#undef catan
#endif
double complex (*foo)(double complex) = catan;
int main(void) { return 0; }
