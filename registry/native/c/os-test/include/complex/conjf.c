#include <complex.h>
#ifdef conjf
#undef conjf
#endif
float complex (*foo)(float complex) = conjf;
int main(void) { return 0; }
