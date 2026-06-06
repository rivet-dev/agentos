#include <math.h>
#ifdef logb
#undef logb
#endif
double (*foo)(double) = logb;
int main(void) { return 0; }
