#include <math.h>
#ifdef remainderf
#undef remainderf
#endif
float (*foo)(float, float) = remainderf;
int main(void) { return 0; }
