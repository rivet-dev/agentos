#include <math.h>
#ifdef lgammal
#undef lgammal
#endif
long double (*foo)(long double) = lgammal;
int main(void) { return 0; }
