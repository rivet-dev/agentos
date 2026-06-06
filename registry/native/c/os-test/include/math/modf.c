#include <math.h>
#ifdef modf
#undef modf
#endif
double (*foo)(double, double *) = modf;
int main(void) { return 0; }
