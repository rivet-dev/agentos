#include <complex.h>
#ifdef cprojl
#undef cprojl
#endif
long double complex (*foo)(long double complex) = cprojl;
int main(void) { return 0; }
