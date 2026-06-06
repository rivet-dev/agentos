#include <complex.h>
#ifdef catanf
#undef catanf
#endif
float complex (*foo)(float complex) = catanf;
int main(void) { return 0; }
