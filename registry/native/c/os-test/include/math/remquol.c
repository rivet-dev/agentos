#include <math.h>
#ifdef remquol
#undef remquol
#endif
long double (*foo)(long double, long double, int *) = remquol;
int main(void) { return 0; }
