#include <math.h>
#ifdef log10l
#undef log10l
#endif
long double (*foo)(long double) = log10l;
int main(void) { return 0; }
