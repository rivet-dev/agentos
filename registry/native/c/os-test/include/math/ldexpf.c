#include <math.h>
#ifdef ldexpf
#undef ldexpf
#endif
float (*foo)(float, int) = ldexpf;
int main(void) { return 0; }
