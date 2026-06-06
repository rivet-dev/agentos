#include <math.h>
#ifdef fabsl
#undef fabsl
#endif
long double (*foo)(long double) = fabsl;
int main(void) { return 0; }
