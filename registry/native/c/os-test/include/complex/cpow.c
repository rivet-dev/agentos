#include <complex.h>
#ifdef cpow
#undef cpow
#endif
double complex (*foo)(double complex, double complex) = cpow;
int main(void) { return 0; }
