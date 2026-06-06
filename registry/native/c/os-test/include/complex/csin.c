#include <complex.h>
#ifdef csin
#undef csin
#endif
double complex (*foo)(double complex) = csin;
int main(void) { return 0; }
