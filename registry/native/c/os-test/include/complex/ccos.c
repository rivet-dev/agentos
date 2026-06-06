#include <complex.h>
#ifdef ccos
#undef ccos
#endif
double complex (*foo)(double complex) = ccos;
int main(void) { return 0; }
