#include <math.h>
#ifdef cbrtf
#undef cbrtf
#endif
float (*foo)(float) = cbrtf;
int main(void) { return 0; }
