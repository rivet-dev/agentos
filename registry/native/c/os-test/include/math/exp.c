#include <math.h>
#ifdef exp
#undef exp
#endif
double (*foo)(double) = exp;
int main(void) { return 0; }
