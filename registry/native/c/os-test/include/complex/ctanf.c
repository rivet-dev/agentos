#include <complex.h>
#ifdef ctanf
#undef ctanf
#endif
float complex (*foo)(float complex) = ctanf;
int main(void) { return 0; }
