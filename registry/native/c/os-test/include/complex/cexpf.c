#include <complex.h>
#ifdef cexpf
#undef cexpf
#endif
float complex (*foo)(float complex) = cexpf;
int main(void) { return 0; }
