#include <complex.h>
#ifdef cpowf
#undef cpowf
#endif
float complex (*foo)(float complex, float complex) = cpowf;
int main(void) { return 0; }
