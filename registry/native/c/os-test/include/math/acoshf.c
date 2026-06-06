#include <math.h>
#ifdef acoshf
#undef acoshf
#endif
float (*foo)(float) = acoshf;
int main(void) { return 0; }
