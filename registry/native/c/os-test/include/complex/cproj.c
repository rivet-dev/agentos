#include <complex.h>
#ifdef cproj
#undef cproj
#endif
double complex (*foo)(double complex) = cproj;
int main(void) { return 0; }
