#include <complex.h>
#ifdef cabsf
#undef cabsf
#endif
float (*foo)(float complex) = cabsf;
int main(void) { return 0; }
