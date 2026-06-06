#include <math.h>
#ifdef logbf
#undef logbf
#endif
float (*foo)(float) = logbf;
int main(void) { return 0; }
