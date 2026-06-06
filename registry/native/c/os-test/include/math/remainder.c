#include <math.h>
#ifdef remainder
#undef remainder
#endif
double (*foo)(double, double) = remainder;
int main(void) { return 0; }
