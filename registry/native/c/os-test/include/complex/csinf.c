#include <complex.h>
#ifdef csinf
#undef csinf
#endif
float complex (*foo)(float complex) = csinf;
int main(void) { return 0; }
