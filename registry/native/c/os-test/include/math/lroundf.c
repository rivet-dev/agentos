#include <math.h>
#ifdef lroundf
#undef lroundf
#endif
long (*foo)(float) = lroundf;
int main(void) { return 0; }
