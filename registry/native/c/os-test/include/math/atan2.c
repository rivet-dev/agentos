#include <math.h>
#ifdef atan2
#undef atan2
#endif
double (*foo)(double, double) = atan2;
int main(void) { return 0; }
