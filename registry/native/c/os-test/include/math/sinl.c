#include <math.h>
#ifdef sinl
#undef sinl
#endif
long double (*foo)(long double) = sinl;
int main(void) { return 0; }
