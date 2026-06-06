#include <math.h>
#ifdef log10
#undef log10
#endif
double (*foo)(double) = log10;
int main(void) { return 0; }
