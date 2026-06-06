#include <math.h>
#ifdef atan2f
#undef atan2f
#endif
float (*foo)(float, float) = atan2f;
int main(void) { return 0; }
