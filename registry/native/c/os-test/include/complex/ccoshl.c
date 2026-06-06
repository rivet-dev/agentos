#include <complex.h>
#ifdef ccoshl
#undef ccoshl
#endif
long double complex (*foo)(long double complex) = ccoshl;
int main(void) { return 0; }
