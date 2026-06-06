#include <math.h>
#ifdef acoshl
#undef acoshl
#endif
long double (*foo)(long double) = acoshl;
int main(void) { return 0; }
