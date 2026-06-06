#include <complex.h>
#ifdef creal
#undef creal
#endif
double (*foo)(double complex) = creal;
int main(void) { return 0; }
