#include <math.h>
#ifdef asinh
#undef asinh
#endif
double (*foo)(double) = asinh;
int main(void) { return 0; }
