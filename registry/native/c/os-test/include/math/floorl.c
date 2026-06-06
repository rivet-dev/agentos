#include <math.h>
#ifdef floorl
#undef floorl
#endif
long double (*foo)(long double) = floorl;
int main(void) { return 0; }
