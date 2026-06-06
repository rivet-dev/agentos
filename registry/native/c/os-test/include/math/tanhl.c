#include <math.h>
#ifdef tanhl
#undef tanhl
#endif
long double (*foo)(long double) = tanhl;
int main(void) { return 0; }
