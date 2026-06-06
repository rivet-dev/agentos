#include <complex.h>
#ifdef casinhl
#undef casinhl
#endif
long double complex (*foo)(long double complex) = casinhl;
int main(void) { return 0; }
