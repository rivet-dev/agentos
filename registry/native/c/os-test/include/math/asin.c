#include <math.h>
#ifdef asin
#undef asin
#endif
double (*foo)(double) = asin;
int main(void) { return 0; }
