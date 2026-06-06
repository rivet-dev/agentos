#include <math.h>
#ifdef atan2l
#undef atan2l
#endif
long double (*foo)(long double, long double) = atan2l;
int main(void) { return 0; }
