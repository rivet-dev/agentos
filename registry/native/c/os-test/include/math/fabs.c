#include <math.h>
#ifdef fabs
#undef fabs
#endif
double (*foo)(double) = fabs;
int main(void) { return 0; }
