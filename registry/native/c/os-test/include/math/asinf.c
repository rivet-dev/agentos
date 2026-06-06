#include <math.h>
#ifdef asinf
#undef asinf
#endif
float (*foo)(float) = asinf;
int main(void) { return 0; }
