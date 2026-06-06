#include <math.h>
#ifdef scalbn
#undef scalbn
#endif
double (*foo)(double, int) = scalbn;
int main(void) { return 0; }
