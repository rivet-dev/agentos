#include <math.h>
#ifdef logf
#undef logf
#endif
float (*foo)(float) = logf;
int main(void) { return 0; }
