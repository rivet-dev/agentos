#include <math.h>
#ifdef nextafterl
#undef nextafterl
#endif
long double (*foo)(long double, long double) = nextafterl;
int main(void) { return 0; }
