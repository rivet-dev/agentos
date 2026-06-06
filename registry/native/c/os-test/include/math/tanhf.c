#include <math.h>
#ifdef tanhf
#undef tanhf
#endif
float (*foo)(float) = tanhf;
int main(void) { return 0; }
