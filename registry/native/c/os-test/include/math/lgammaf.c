#include <math.h>
#ifdef lgammaf
#undef lgammaf
#endif
float (*foo)(float) = lgammaf;
int main(void) { return 0; }
