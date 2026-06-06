#include <math.h>
#ifdef fminf
#undef fminf
#endif
float (*foo)(float, float) = fminf;
int main(void) { return 0; }
