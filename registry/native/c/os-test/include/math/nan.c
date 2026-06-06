#include <math.h>
#ifdef nan
#undef nan
#endif
double (*foo)(const char *) = nan;
int main(void) { return 0; }
