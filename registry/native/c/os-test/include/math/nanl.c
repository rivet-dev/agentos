#include <math.h>
#ifdef nanl
#undef nanl
#endif
long double (*foo)(const char *) = nanl;
int main(void) { return 0; }
