#include <math.h>
#ifdef atanl
#undef atanl
#endif
long double (*foo)(long double) = atanl;
int main(void) { return 0; }
