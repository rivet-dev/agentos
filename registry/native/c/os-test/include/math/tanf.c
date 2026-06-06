#include <math.h>
#ifdef tanf
#undef tanf
#endif
float (*foo)(float) = tanf;
int main(void) { return 0; }
