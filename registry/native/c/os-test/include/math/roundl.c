#include <math.h>
#ifdef roundl
#undef roundl
#endif
long double (*foo)(long double) = roundl;
int main(void) { return 0; }
