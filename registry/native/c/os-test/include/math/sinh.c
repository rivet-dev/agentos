#include <math.h>
#ifdef sinh
#undef sinh
#endif
double (*foo)(double) = sinh;
int main(void) { return 0; }
