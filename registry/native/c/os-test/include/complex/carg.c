#include <complex.h>
#ifdef carg
#undef carg
#endif
double (*foo)(double complex) = carg;
int main(void) { return 0; }
