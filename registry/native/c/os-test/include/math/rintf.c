#include <math.h>
#ifdef rintf
#undef rintf
#endif
float (*foo)(float) = rintf;
int main(void) { return 0; }
