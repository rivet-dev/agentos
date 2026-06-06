#include <complex.h>
#ifdef cacosh
#undef cacosh
#endif
double complex (*foo)(double complex) = cacosh;
int main(void) { return 0; }
