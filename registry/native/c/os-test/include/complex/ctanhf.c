#include <complex.h>
#ifdef ctanhf
#undef ctanhf
#endif
float complex (*foo)(float complex) = ctanhf;
int main(void) { return 0; }
