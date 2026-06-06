#include <math.h>
#ifdef acosl
#undef acosl
#endif
long double (*foo)(long double) = acosl;
int main(void) { return 0; }
