#include <math.h>
#ifdef frexpf
#undef frexpf
#endif
float (*foo)(float, int *) = frexpf;
int main(void) { return 0; }
