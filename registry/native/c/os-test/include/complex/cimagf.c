#include <complex.h>
#ifdef cimagf
#undef cimagf
#endif
float (*foo)(float complex) = cimagf;
int main(void) { return 0; }
