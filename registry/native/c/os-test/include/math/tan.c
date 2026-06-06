#include <math.h>
#ifdef tan
#undef tan
#endif
double (*foo)(double) = tan;
int main(void) { return 0; }
