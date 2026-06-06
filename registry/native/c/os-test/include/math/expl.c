#include <math.h>
#ifdef expl
#undef expl
#endif
long double (*foo)(long double) = expl;
int main(void) { return 0; }
