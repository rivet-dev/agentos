#include <complex.h>
#ifdef csinhf
#undef csinhf
#endif
float complex (*foo)(float complex) = csinhf;
int main(void) { return 0; }
