#include <math.h>
#ifdef lrint
#undef lrint
#endif
long (*foo)(double) = lrint;
int main(void) { return 0; }
