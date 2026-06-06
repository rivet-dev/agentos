#include <math.h>
#ifdef erfc
#undef erfc
#endif
double (*foo)(double) = erfc;
int main(void) { return 0; }
