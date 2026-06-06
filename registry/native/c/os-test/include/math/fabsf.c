#include <math.h>
#ifdef fabsf
#undef fabsf
#endif
float (*foo)(float) = fabsf;
int main(void) { return 0; }
