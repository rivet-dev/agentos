#include <math.h>
#ifdef remquo
#undef remquo
#endif
double (*foo)(double, double, int *) = remquo;
int main(void) { return 0; }
