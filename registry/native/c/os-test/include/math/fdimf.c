#include <math.h>
#ifdef fdimf
#undef fdimf
#endif
float (*foo)(float, float) = fdimf;
int main(void) { return 0; }
