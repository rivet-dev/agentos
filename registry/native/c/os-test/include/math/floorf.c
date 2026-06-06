#include <math.h>
#ifdef floorf
#undef floorf
#endif
float (*foo)(float) = floorf;
int main(void) { return 0; }
