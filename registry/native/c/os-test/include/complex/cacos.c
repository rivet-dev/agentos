#include <complex.h>
#ifdef cacos
#undef cacos
#endif
double complex (*foo)(double complex) = cacos;
int main(void) { return 0; }
