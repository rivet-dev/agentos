#include <complex.h>
#ifdef casinl
#undef casinl
#endif
long double complex (*foo)(long double complex) = casinl;
int main(void) { return 0; }
