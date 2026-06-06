#include <math.h>
#ifdef cosh
#undef cosh
#endif
double (*foo)(double) = cosh;
int main(void) { return 0; }
