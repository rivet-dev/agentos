#include <math.h>
#ifdef fmaxl
#undef fmaxl
#endif
long double (*foo)(long double, long double) = fmaxl;
int main(void) { return 0; }
