#include <math.h>
#ifdef pow
#undef pow
#endif
double (*foo)(double, double) = pow;
int main(void) { return 0; }
