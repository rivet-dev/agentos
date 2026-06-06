#include <complex.h>
#ifdef conjl
#undef conjl
#endif
long double complex (*foo)(long double complex) = conjl;
int main(void) { return 0; }
