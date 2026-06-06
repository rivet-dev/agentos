#include <complex.h>
#ifdef catanhl
#undef catanhl
#endif
long double complex (*foo)(long double complex) = catanhl;
int main(void) { return 0; }
