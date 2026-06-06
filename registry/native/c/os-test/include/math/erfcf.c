#include <math.h>
#ifdef erfcf
#undef erfcf
#endif
float (*foo)(float) = erfcf;
int main(void) { return 0; }
