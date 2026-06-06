#include <math.h>
#ifdef expf
#undef expf
#endif
float (*foo)(float) = expf;
int main(void) { return 0; }
