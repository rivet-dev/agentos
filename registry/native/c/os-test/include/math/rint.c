#include <math.h>
#ifdef rint
#undef rint
#endif
double (*foo)(double) = rint;
int main(void) { return 0; }
