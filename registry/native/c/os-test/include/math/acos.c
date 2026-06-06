#include <math.h>
#ifdef acos
#undef acos
#endif
double (*foo)(double) = acos;
int main(void) { return 0; }
