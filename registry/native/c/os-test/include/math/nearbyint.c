#include <math.h>
#ifdef nearbyint
#undef nearbyint
#endif
double (*foo)(double) = nearbyint;
int main(void) { return 0; }
