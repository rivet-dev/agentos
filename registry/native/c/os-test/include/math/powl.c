#include <math.h>
#ifdef powl
#undef powl
#endif
long double (*foo)(long double, long double) = powl;
int main(void) { return 0; }
