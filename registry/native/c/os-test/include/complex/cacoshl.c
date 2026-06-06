#include <complex.h>
#ifdef cacoshl
#undef cacoshl
#endif
long double complex (*foo)(long double complex) = cacoshl;
int main(void) { return 0; }
