#include <math.h>
#ifdef sinhf
#undef sinhf
#endif
float (*foo)(float) = sinhf;
int main(void) { return 0; }
