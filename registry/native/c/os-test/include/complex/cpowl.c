#include <complex.h>
#ifdef cpowl
#undef cpowl
#endif
long double complex (*foo)(long double complex, long double complex) = cpowl;
int main(void) { return 0; }
