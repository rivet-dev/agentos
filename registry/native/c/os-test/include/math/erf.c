#include <math.h>
#ifdef erf
#undef erf
#endif
double (*foo)(double) = erf;
int main(void) { return 0; }
