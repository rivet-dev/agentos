#include <math.h>
#ifdef lrintl
#undef lrintl
#endif
long (*foo)(long double) = lrintl;
int main(void) { return 0; }
