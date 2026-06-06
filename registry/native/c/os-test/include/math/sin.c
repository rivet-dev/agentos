#include <math.h>
#ifdef sin
#undef sin
#endif
double (*foo)(double) = sin;
int main(void) { return 0; }
