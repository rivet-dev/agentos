#include <math.h>
#ifdef exp2f
#undef exp2f
#endif
float (*foo)(float) = exp2f;
int main(void) { return 0; }
