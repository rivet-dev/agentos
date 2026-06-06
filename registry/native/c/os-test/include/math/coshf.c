#include <math.h>
#ifdef coshf
#undef coshf
#endif
float (*foo)(float) = coshf;
int main(void) { return 0; }
