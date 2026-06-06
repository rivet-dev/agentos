#include <complex.h>
#ifdef conj
#undef conj
#endif
double complex (*foo)(double complex) = conj;
int main(void) { return 0; }
