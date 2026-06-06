#include <math.h>
#ifdef acosf
#undef acosf
#endif
float (*foo)(float) = acosf;
int main(void) { return 0; }
