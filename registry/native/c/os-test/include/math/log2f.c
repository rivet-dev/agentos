#include <math.h>
#ifdef log2f
#undef log2f
#endif
float (*foo)(float) = log2f;
int main(void) { return 0; }
