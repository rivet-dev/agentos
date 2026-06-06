#include <math.h>
#ifdef ldexpl
#undef ldexpl
#endif
long double (*foo)(long double, int) = ldexpl;
int main(void) { return 0; }
