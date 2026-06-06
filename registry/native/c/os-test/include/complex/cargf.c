#include <complex.h>
#ifdef cargf
#undef cargf
#endif
float (*foo)(float complex) = cargf;
int main(void) { return 0; }
