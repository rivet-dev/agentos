#include <math.h>
#ifdef expm1f
#undef expm1f
#endif
float (*foo)(float) = expm1f;
int main(void) { return 0; }
