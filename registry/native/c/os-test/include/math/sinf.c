#include <math.h>
#ifdef sinf
#undef sinf
#endif
float (*foo)(float) = sinf;
int main(void) { return 0; }
