#include <math.h>
#ifdef lroundl
#undef lroundl
#endif
long (*foo)(long double) = lroundl;
int main(void) { return 0; }
