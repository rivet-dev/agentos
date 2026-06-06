#include <math.h>
#ifdef log
#undef log
#endif
double (*foo)(double) = log;
int main(void) { return 0; }
