#include <complex.h>
#ifdef clogf
#undef clogf
#endif
float complex (*foo)(float complex) = clogf;
int main(void) { return 0; }
