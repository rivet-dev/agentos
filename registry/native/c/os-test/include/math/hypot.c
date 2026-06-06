#include <math.h>
#ifdef hypot
#undef hypot
#endif
double (*foo)(double, double) = hypot;
int main(void) { return 0; }
