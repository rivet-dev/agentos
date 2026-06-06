#include <math.h>
#ifdef fmodl
#undef fmodl
#endif
long double (*foo)(long double, long double) = fmodl;
int main(void) { return 0; }
