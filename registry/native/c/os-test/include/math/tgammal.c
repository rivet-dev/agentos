#include <math.h>
#ifdef tgammal
#undef tgammal
#endif
long double (*foo)(long double) = tgammal;
int main(void) { return 0; }
