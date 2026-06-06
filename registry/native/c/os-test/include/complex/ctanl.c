#include <complex.h>
#ifdef ctanl
#undef ctanl
#endif
long double complex (*foo)(long double complex) = ctanl;
int main(void) { return 0; }
