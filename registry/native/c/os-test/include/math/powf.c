#include <math.h>
#ifdef powf
#undef powf
#endif
float (*foo)(float, float) = powf;
int main(void) { return 0; }
