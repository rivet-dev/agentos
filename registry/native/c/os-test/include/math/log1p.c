#include <math.h>
#ifdef log1p
#undef log1p
#endif
double (*foo)(double) = log1p;
int main(void) { return 0; }
