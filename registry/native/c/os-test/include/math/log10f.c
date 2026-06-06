#include <math.h>
#ifdef log10f
#undef log10f
#endif
float (*foo)(float) = log10f;
int main(void) { return 0; }
