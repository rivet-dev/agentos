#include <math.h>
#ifdef erfcl
#undef erfcl
#endif
long double (*foo)(long double) = erfcl;
int main(void) { return 0; }
