#include <complex.h>
#ifdef ctanhl
#undef ctanhl
#endif
long double complex (*foo)(long double complex) = ctanhl;
int main(void) { return 0; }
