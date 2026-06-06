#include <complex.h>
#ifdef cacosl
#undef cacosl
#endif
long double complex (*foo)(long double complex) = cacosl;
int main(void) { return 0; }
