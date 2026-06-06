#include <math.h>
#ifdef truncl
#undef truncl
#endif
long double (*foo)(long double) = truncl;
int main(void) { return 0; }
