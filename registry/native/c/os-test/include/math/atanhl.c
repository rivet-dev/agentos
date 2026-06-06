#include <math.h>
#ifdef atanhl
#undef atanhl
#endif
long double (*foo)(long double) = atanhl;
int main(void) { return 0; }
