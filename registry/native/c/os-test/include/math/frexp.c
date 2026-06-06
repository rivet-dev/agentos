#include <math.h>
#ifdef frexp
#undef frexp
#endif
double (*foo)(double, int *) = frexp;
int main(void) { return 0; }
