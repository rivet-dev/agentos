#include <math.h>
#ifdef fmal
#undef fmal
#endif
long double (*foo)(long double, long double, long double) = fmal;
int main(void) { return 0; }
