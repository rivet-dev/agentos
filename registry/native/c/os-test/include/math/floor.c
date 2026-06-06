#include <math.h>
#ifdef floor
#undef floor
#endif
double (*foo)(double) = floor;
int main(void) { return 0; }
