#include <math.h>
#ifdef cos
#undef cos
#endif
double (*foo)(double) = cos;
int main(void) { return 0; }
