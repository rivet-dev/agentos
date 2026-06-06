#include <math.h>
#ifdef round
#undef round
#endif
double (*foo)(double) = round;
int main(void) { return 0; }
