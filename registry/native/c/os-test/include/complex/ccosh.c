#include <complex.h>
#ifdef ccosh
#undef ccosh
#endif
double complex (*foo)(double complex) = ccosh;
int main(void) { return 0; }
