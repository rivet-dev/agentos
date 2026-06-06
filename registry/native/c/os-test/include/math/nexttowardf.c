#include <math.h>
#ifdef nexttowardf
#undef nexttowardf
#endif
float (*foo)(float, long double) = nexttowardf;
int main(void) { return 0; }
