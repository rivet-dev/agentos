#include <math.h>
#ifdef ceilf
#undef ceilf
#endif
float (*foo)(float) = ceilf;
int main(void) { return 0; }
