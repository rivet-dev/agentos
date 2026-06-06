#include <math.h>
#ifdef copysign
#undef copysign
#endif
double (*foo)(double, double) = copysign;
int main(void) { return 0; }
