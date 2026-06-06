#include <complex.h>
#ifdef csqrtl
#undef csqrtl
#endif
long double complex (*foo)(long double complex) = csqrtl;
int main(void) { return 0; }
