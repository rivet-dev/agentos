#include <math.h>
#ifdef fmod
#undef fmod
#endif
double (*foo)(double, double) = fmod;
int main(void) { return 0; }
