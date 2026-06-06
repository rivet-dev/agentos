#include <math.h>
#ifdef hypotf
#undef hypotf
#endif
float (*foo)(float, float) = hypotf;
int main(void) { return 0; }
