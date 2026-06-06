#include <math.h>
#ifdef asinhl
#undef asinhl
#endif
long double (*foo)(long double) = asinhl;
int main(void) { return 0; }
