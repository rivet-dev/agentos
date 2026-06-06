#include <math.h>
#ifdef logbl
#undef logbl
#endif
long double (*foo)(long double) = logbl;
int main(void) { return 0; }
