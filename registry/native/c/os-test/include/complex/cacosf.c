#include <complex.h>
#ifdef cacosf
#undef cacosf
#endif
float complex (*foo)(float complex) = cacosf;
int main(void) { return 0; }
