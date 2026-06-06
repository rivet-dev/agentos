#include <math.h>
#ifdef lgamma
#undef lgamma
#endif
double (*foo)(double) = lgamma;
int main(void) { return 0; }
