#include <math.h>
#ifdef erff
#undef erff
#endif
float (*foo)(float) = erff;
int main(void) { return 0; }
