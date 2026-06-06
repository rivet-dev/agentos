#include <math.h>
#ifdef exp2
#undef exp2
#endif
double (*foo)(double) = exp2;
int main(void) { return 0; }
