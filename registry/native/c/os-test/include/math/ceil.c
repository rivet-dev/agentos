#include <math.h>
#ifdef ceil
#undef ceil
#endif
double (*foo)(double) = ceil;
int main(void) { return 0; }
