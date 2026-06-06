#include <complex.h>
#ifdef catanh
#undef catanh
#endif
double complex (*foo)(double complex) = catanh;
int main(void) { return 0; }
