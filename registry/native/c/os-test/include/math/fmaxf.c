#include <math.h>
#ifdef fmaxf
#undef fmaxf
#endif
float (*foo)(float, float) = fmaxf;
int main(void) { return 0; }
