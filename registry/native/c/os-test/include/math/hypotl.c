#include <math.h>
#ifdef hypotl
#undef hypotl
#endif
long double (*foo)(long double, long double) = hypotl;
int main(void) { return 0; }
