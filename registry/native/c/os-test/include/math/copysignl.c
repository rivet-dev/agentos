#include <math.h>
#ifdef copysignl
#undef copysignl
#endif
long double (*foo)(long double, long double) = copysignl;
int main(void) { return 0; }
