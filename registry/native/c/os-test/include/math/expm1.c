#include <math.h>
#ifdef expm1
#undef expm1
#endif
double (*foo)(double) = expm1;
int main(void) { return 0; }
