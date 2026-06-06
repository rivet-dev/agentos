#include <math.h>
#ifdef lround
#undef lround
#endif
long (*foo)(double) = lround;
int main(void) { return 0; }
