#include <complex.h>
#ifdef casinf
#undef casinf
#endif
float complex (*foo)(float complex) = casinf;
int main(void) { return 0; }
