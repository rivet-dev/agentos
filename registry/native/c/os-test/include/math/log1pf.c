#include <math.h>
#ifdef log1pf
#undef log1pf
#endif
float (*foo)(float) = log1pf;
int main(void) { return 0; }
