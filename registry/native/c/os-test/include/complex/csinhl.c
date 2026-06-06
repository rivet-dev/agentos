#include <complex.h>
#ifdef csinhl
#undef csinhl
#endif
long double complex (*foo)(long double complex) = csinhl;
int main(void) { return 0; }
