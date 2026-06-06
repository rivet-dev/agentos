#include <math.h>
#ifdef fmodf
#undef fmodf
#endif
float (*foo)(float, float) = fmodf;
int main(void) { return 0; }
