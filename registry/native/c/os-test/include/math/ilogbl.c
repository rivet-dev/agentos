#include <math.h>
#ifdef ilogbl
#undef ilogbl
#endif
int (*foo)(long double) = ilogbl;
int main(void) { return 0; }
