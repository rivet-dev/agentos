#include <math.h>
#ifdef fma
#undef fma
#endif
double (*foo)(double, double, double) = fma;
int main(void) { return 0; }
