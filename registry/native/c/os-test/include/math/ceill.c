#include <math.h>
#ifdef ceill
#undef ceill
#endif
long double (*foo)(long double) = ceill;
int main(void) { return 0; }
