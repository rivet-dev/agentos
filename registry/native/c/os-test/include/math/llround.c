#include <math.h>
#ifdef llround
#undef llround
#endif
long long (*foo)(double) = llround;
int main(void) { return 0; }
