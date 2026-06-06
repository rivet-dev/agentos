#include <math.h>
#ifdef expm1l
#undef expm1l
#endif
long double (*foo)(long double) = expm1l;
int main(void) { return 0; }
