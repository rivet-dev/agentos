#include <math.h>
#ifdef nextafterf
#undef nextafterf
#endif
float (*foo)(float, float) = nextafterf;
int main(void) { return 0; }
