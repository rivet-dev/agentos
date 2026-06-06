#include <math.h>
#ifdef sqrt
#undef sqrt
#endif
double (*foo)(double) = sqrt;
int main(void) { return 0; }
