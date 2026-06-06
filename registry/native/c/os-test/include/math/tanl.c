#include <math.h>
#ifdef tanl
#undef tanl
#endif
long double (*foo)(long double) = tanl;
int main(void) { return 0; }
