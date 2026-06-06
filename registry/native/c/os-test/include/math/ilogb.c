#include <math.h>
#ifdef ilogb
#undef ilogb
#endif
int (*foo)(double) = ilogb;
int main(void) { return 0; }
