#include <math.h>
#ifdef fmaf
#undef fmaf
#endif
float (*foo)(float, float, float) = fmaf;
int main(void) { return 0; }
