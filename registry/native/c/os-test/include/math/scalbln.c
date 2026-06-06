#include <math.h>
#ifdef scalbln
#undef scalbln
#endif
double (*foo)(double, long) = scalbln;
int main(void) { return 0; }
