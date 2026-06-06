#include <math.h>
#ifdef fmax
#undef fmax
#endif
double (*foo)(double, double) = fmax;
int main(void) { return 0; }
