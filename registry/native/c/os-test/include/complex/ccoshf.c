#include <complex.h>
#ifdef ccoshf
#undef ccoshf
#endif
float complex (*foo)(float complex) = ccoshf;
int main(void) { return 0; }
