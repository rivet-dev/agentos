#include <complex.h>
#ifdef cargl
#undef cargl
#endif
long double (*foo)(long double complex) = cargl;
int main(void) { return 0; }
