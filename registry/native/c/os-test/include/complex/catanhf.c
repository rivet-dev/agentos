#include <complex.h>
#ifdef catanhf
#undef catanhf
#endif
float complex (*foo)(float complex) = catanhf;
int main(void) { return 0; }
