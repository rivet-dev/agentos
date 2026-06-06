#include <math.h>
#ifdef ldexp
#undef ldexp
#endif
double (*foo)(double, int) = ldexp;
int main(void) { return 0; }
