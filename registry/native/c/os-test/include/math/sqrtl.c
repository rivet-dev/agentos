#include <math.h>
#ifdef sqrtl
#undef sqrtl
#endif
long double (*foo)(long double) = sqrtl;
int main(void) { return 0; }
