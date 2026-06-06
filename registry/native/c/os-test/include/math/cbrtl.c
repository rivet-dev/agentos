#include <math.h>
#ifdef cbrtl
#undef cbrtl
#endif
long double (*foo)(long double) = cbrtl;
int main(void) { return 0; }
