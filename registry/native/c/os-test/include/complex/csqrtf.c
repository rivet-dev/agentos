#include <complex.h>
#ifdef csqrtf
#undef csqrtf
#endif
float complex (*foo)(float complex) = csqrtf;
int main(void) { return 0; }
