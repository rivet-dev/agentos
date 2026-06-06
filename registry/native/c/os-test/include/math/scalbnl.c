#include <math.h>
#ifdef scalbnl
#undef scalbnl
#endif
long double (*foo)(long double, int) = scalbnl;
int main(void) { return 0; }
