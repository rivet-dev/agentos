#include <math.h>
#ifdef truncf
#undef truncf
#endif
float (*foo)(float) = truncf;
int main(void) { return 0; }
