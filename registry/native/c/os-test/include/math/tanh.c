#include <math.h>
#ifdef tanh
#undef tanh
#endif
double (*foo)(double) = tanh;
int main(void) { return 0; }
