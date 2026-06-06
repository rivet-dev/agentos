#include <complex.h>
#ifdef ccosl
#undef ccosl
#endif
long double complex (*foo)(long double complex) = ccosl;
int main(void) { return 0; }
