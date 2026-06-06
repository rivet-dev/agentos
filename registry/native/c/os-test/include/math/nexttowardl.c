#include <math.h>
#ifdef nexttowardl
#undef nexttowardl
#endif
long double (*foo)(long double, long double) = nexttowardl;
int main(void) { return 0; }
