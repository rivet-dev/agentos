#include <math.h>
#ifdef atanf
#undef atanf
#endif
float (*foo)(float) = atanf;
int main(void) { return 0; }
