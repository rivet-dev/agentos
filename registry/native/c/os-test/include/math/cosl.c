#include <math.h>
#ifdef cosl
#undef cosl
#endif
long double (*foo)(long double) = cosl;
int main(void) { return 0; }
