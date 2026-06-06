#include <math.h>
#ifdef log2l
#undef log2l
#endif
long double (*foo)(long double) = log2l;
int main(void) { return 0; }
