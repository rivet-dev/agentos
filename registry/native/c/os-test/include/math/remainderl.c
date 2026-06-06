#include <math.h>
#ifdef remainderl
#undef remainderl
#endif
long double (*foo)(long double, long double) = remainderl;
int main(void) { return 0; }
