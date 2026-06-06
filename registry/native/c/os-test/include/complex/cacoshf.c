#include <complex.h>
#ifdef cacoshf
#undef cacoshf
#endif
float complex (*foo)(float complex) = cacoshf;
int main(void) { return 0; }
