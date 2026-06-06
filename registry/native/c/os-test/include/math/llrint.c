#include <math.h>
#ifdef llrint
#undef llrint
#endif
long long (*foo)(double) = llrint;
int main(void) { return 0; }
