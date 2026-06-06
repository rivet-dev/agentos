#include <math.h>
#ifdef lrintf
#undef lrintf
#endif
long (*foo)(float) = lrintf;
int main(void) { return 0; }
