#include <math.h>
#ifdef ilogbf
#undef ilogbf
#endif
int (*foo)(float) = ilogbf;
int main(void) { return 0; }
