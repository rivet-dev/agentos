#include <math.h>
#ifdef llroundf
#undef llroundf
#endif
long long (*foo)(float) = llroundf;
int main(void) { return 0; }
