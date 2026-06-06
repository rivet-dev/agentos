#include <complex.h>
#ifdef cabs
#undef cabs
#endif
double (*foo)(double complex) = cabs;
int main(void) { return 0; }
