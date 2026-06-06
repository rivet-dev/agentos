#include <math.h>
#ifdef exp2l
#undef exp2l
#endif
long double (*foo)(long double) = exp2l;
int main(void) { return 0; }
