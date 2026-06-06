#include <math.h>
#ifdef asinhf
#undef asinhf
#endif
float (*foo)(float) = asinhf;
int main(void) { return 0; }
