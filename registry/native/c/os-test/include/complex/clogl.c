#include <complex.h>
#ifdef clogl
#undef clogl
#endif
long double complex (*foo)(long double complex) = clogl;
int main(void) { return 0; }
