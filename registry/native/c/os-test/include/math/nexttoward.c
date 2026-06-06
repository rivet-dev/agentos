#include <math.h>
#ifdef nexttoward
#undef nexttoward
#endif
double (*foo)(double, long double) = nexttoward;
int main(void) { return 0; }
