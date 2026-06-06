#include <math.h>
#ifdef tgamma
#undef tgamma
#endif
double (*foo)(double) = tgamma;
int main(void) { return 0; }
