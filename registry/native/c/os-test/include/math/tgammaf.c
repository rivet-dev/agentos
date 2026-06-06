#include <math.h>
#ifdef tgammaf
#undef tgammaf
#endif
float (*foo)(float) = tgammaf;
int main(void) { return 0; }
