#include <math.h>
#ifdef scalblnl
#undef scalblnl
#endif
long double (*foo)(long double, long) = scalblnl;
int main(void) { return 0; }
