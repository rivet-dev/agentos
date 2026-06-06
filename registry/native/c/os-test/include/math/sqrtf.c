#include <math.h>
#ifdef sqrtf
#undef sqrtf
#endif
float (*foo)(float) = sqrtf;
int main(void) { return 0; }
