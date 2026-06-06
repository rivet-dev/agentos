#include <math.h>
#ifdef sinhl
#undef sinhl
#endif
long double (*foo)(long double) = sinhl;
int main(void) { return 0; }
