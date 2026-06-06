#include <math.h>
#ifdef fdim
#undef fdim
#endif
double (*foo)(double, double) = fdim;
int main(void) { return 0; }
