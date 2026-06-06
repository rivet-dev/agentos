#include <math.h>
#ifdef log2
#undef log2
#endif
double (*foo)(double) = log2;
int main(void) { return 0; }
