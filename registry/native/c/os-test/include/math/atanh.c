#include <math.h>
#ifdef atanh
#undef atanh
#endif
double (*foo)(double) = atanh;
int main(void) { return 0; }
