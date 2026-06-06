#include <math.h>
#ifdef fmin
#undef fmin
#endif
double (*foo)(double, double) = fmin;
int main(void) { return 0; }
