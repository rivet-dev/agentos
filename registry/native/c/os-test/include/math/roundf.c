#include <math.h>
#ifdef roundf
#undef roundf
#endif
float (*foo)(float) = roundf;
int main(void) { return 0; }
