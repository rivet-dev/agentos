#include <complex.h>
#ifdef cexpl
#undef cexpl
#endif
long double complex (*foo)(long double complex) = cexpl;
int main(void) { return 0; }
