#include <math.h>
#ifdef copysignf
#undef copysignf
#endif
float (*foo)(float, float) = copysignf;
int main(void) { return 0; }
