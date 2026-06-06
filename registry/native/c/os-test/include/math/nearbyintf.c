#include <math.h>
#ifdef nearbyintf
#undef nearbyintf
#endif
float (*foo)(float) = nearbyintf;
int main(void) { return 0; }
