#include <math.h>
#ifdef log1pl
#undef log1pl
#endif
long double (*foo)(long double) = log1pl;
int main(void) { return 0; }
