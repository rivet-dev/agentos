#include <math.h>
#ifdef asinl
#undef asinl
#endif
long double (*foo)(long double) = asinl;
int main(void) { return 0; }
