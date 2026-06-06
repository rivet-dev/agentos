#include <math.h>
#ifdef fdiml
#undef fdiml
#endif
long double (*foo)(long double, long double) = fdiml;
int main(void) { return 0; }
