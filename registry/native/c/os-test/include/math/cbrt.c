#include <math.h>
#ifdef cbrt
#undef cbrt
#endif
double (*foo)(double) = cbrt;
int main(void) { return 0; }
