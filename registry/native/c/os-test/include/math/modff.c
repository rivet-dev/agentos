#include <math.h>
#ifdef modff
#undef modff
#endif
float (*foo)(float, float *) = modff;
int main(void) { return 0; }
