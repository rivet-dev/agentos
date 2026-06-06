#include <math.h>
#ifdef trunc
#undef trunc
#endif
double (*foo)(double) = trunc;
int main(void) { return 0; }
