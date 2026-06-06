#include <complex.h>
#ifdef cabsl
#undef cabsl
#endif
long double (*foo)(long double complex) = cabsl;
int main(void) { return 0; }
