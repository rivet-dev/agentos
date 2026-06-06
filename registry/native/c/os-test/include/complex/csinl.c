#include <complex.h>
#ifdef csinl
#undef csinl
#endif
long double complex (*foo)(long double complex) = csinl;
int main(void) { return 0; }
