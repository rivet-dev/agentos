#include <complex.h>
#ifdef ccosf
#undef ccosf
#endif
float complex (*foo)(float complex) = ccosf;
int main(void) { return 0; }
