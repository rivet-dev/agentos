#include <math.h>
#ifdef nearbyintl
#undef nearbyintl
#endif
long double (*foo)(long double) = nearbyintl;
int main(void) { return 0; }
