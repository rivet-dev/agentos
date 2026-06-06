#include <math.h>
#ifdef nextafter
#undef nextafter
#endif
double (*foo)(double, double) = nextafter;
int main(void) { return 0; }
