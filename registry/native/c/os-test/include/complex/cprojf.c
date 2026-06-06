#include <complex.h>
#ifdef cprojf
#undef cprojf
#endif
float complex (*foo)(float complex) = cprojf;
int main(void) { return 0; }
