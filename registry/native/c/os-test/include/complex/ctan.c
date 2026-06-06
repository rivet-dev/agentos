#include <complex.h>
#ifdef ctan
#undef ctan
#endif
double complex (*foo)(double complex) = ctan;
int main(void) { return 0; }
