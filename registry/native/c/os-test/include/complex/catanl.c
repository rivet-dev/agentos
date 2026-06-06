#include <complex.h>
#ifdef catanl
#undef catanl
#endif
long double complex (*foo)(long double complex) = catanl;
int main(void) { return 0; }
