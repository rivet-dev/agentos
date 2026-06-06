#include <math.h>
#ifdef frexpl
#undef frexpl
#endif
long double (*foo)(long double, int *) = frexpl;
int main(void) { return 0; }
