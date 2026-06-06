#include <complex.h>
#ifdef casin
#undef casin
#endif
double complex (*foo)(double complex) = casin;
int main(void) { return 0; }
