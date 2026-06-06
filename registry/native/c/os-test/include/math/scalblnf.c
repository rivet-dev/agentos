#include <math.h>
#ifdef scalblnf
#undef scalblnf
#endif
float (*foo)(float, long) = scalblnf;
int main(void) { return 0; }
