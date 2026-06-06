#include <math.h>
#ifdef atan
#undef atan
#endif
double (*foo)(double) = atan;
int main(void) { return 0; }
