#include <math.h>
#ifdef remquof
#undef remquof
#endif
float (*foo)(float, float, int *) = remquof;
int main(void) { return 0; }
