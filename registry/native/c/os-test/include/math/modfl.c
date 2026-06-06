#include <math.h>
#ifdef modfl
#undef modfl
#endif
long double (*foo)(long double, long double *) = modfl;
int main(void) { return 0; }
